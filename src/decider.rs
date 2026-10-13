//! Local decision model: openJev (ModernBERT-151M) through ONNX Runtime.
//!
//! A non-autoregressive decision model that scores typed `choice` questions and
//! returns a probability per option without generating text. Accessor uses it
//! for the model router and the mascot tone, replacing the hosted Jev service.
//! The weights are Apache-2.0 (`heman10x/rlcd-modernbert-151m`), dynamically
//! quantized to int8, and fetched on first use like the speech models.

use anyhow::{anyhow, Context, Result};
use ndarray::Array2;
use ort::session::Session;
use ort::value::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tokenizers::Tokenizer;

const LABEL_MARKER: &str = "<<LABEL>>";
const SEP_MARKER: &str = "<<SEP>>";
const ABSTAIN: &str = "insufficient evidence";
const MODEL_FILE: &str = "openjev-modernbert-151m-int8.onnx";
const TOKENIZER_FILE: &str = "openjev-tokenizer.json";
const MODEL_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-modernbert-151m-int8.onnx";
const TOKENIZER_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-tokenizer.json";
const MODEL_SHA256: &str = "11bc7d9a541c17c375ca32af47f26317a3cf98f60f1db60cb6e37a1da785dfde";
const TOKENIZER_SHA256: &str = "8bb449eb0c037aae44115b65905bb339b8f3f74eb37067c19127feb3c0755723";

pub fn dir(assets: &Path) -> PathBuf {
    assets.join("decider")
}

pub fn installed(assets: &Path) -> bool {
    let dir = dir(assets);
    dir.join(MODEL_FILE).is_file() && dir.join(TOKENIZER_FILE).is_file()
}

pub fn size_label() -> String {
    crate::stt_models::size_label(152_724_842 + 3_583_596 + 67_358_322 + 711_494)
}

/// Download the model and tokenizer with checksum verification.
pub async fn download(assets: PathBuf) -> Result<String> {
    let dir = dir(&assets);
    std::fs::create_dir_all(&dir)?;
    crate::stt_models::fetch(MODEL_URL, &dir.join(MODEL_FILE), MODEL_SHA256).await?;
    crate::stt_models::fetch(TOKENIZER_URL, &dir.join(TOKENIZER_FILE), TOKENIZER_SHA256).await?;
    download_gate(&assets).await?;
    Ok("Installed the local decision model.".into())
}

pub struct Choice {
    pub index: usize,
    pub confidence: f32,
    pub abstain: f32,
}

struct Decider {
    session: Session,
    tokenizer: Tokenizer,
}

impl Decider {
    fn load(assets: &Path) -> Result<Decider> {
        crate::audio::prepare_ort()?;
        let dir = dir(assets);
        let tokenizer = Tokenizer::from_file(dir.join(TOKENIZER_FILE))
            .map_err(|e| anyhow!("decision tokenizer: {e}"))?;
        let session = Session::builder()?
            .commit_from_file(dir.join(MODEL_FILE))
            .context("Could not load the decision model")?;
        Ok(Decider { session, tokenizer })
    }

    /// Score `options` (description pairs) for a choice question. Returns the
    /// best option; `abstain` is the model's own "insufficient evidence" mass.
    fn choice(&mut self, question: &str, context: &str, options: &[&str]) -> Result<Choice> {
        let mut prompt = String::new();
        for option in options {
            prompt.push_str(LABEL_MARKER);
            prompt.push_str(option);
        }
        prompt.push_str(LABEL_MARKER);
        prompt.push_str(ABSTAIN);
        prompt.push_str(SEP_MARKER);
        if question.trim().is_empty() {
            prompt.push_str(context);
        } else {
            prompt.push_str(&format!("Question: {question}\n\nContext:\n{context}"));
        }
        let encoded = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow!("decision tokenizer: {e}"))?;
        let len = encoded.get_ids().len();
        let ids: Vec<i64> = encoded.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoded.get_attention_mask().iter().map(|&x| x as i64).collect();
        let ids = Array2::from_shape_vec((1, len), ids)?;
        let mask = Array2::from_shape_vec((1, len), mask)?;
        let outputs = self.session.run(ort::inputs![
            "input_ids" => Value::from_array(ids)?,
            "attention_mask" => Value::from_array(mask)?,
        ])?;
        let (_shape, logits) = outputs[0].try_extract_tensor::<f32>()?;
        let k = options.len();
        let scored = logits
            .get(..=k)
            .context("decision model returned too few candidates")?;
        let max = scored.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = scored.iter().map(|x| (x - max).exp()).collect();
        let sum: f32 = exps.iter().sum();
        let (mut best, mut best_p) = (0, -1.0f32);
        for (i, e) in exps[..k].iter().enumerate() {
            let p = e / sum;
            if p > best_p {
                best_p = p;
                best = i;
            }
        }
        Ok(Choice {
            index: best,
            confidence: best_p,
            abstain: exps[k] / sum,
        })
    }
}

fn decider_assets() -> Option<PathBuf> {
    static ASSETS: OnceLock<Option<PathBuf>> = OnceLock::new();
    ASSETS
        .get_or_init(|| {
            let assets = crate::config::Settings::load().ok()?.assets().ok()?;
            installed(&assets).then_some(assets)
        })
        .clone()
}

/// True when the model is installed and can be loaded lazily.
pub fn available() -> bool {
    decider_assets().is_some()
}

/// Run a `choice` decision, or `None` when the model is absent or fails.
pub fn choice(question: &str, context: &str, options: &[&str]) -> Option<Choice> {
    static DECIDER: OnceLock<Mutex<Option<Decider>>> = OnceLock::new();
    let assets = decider_assets()?;
    let cell = DECIDER.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().ok()?;
    if guard.is_none() {
        match Decider::load(&assets) {
            Ok(decider) => *guard = Some(decider),
            Err(_) => return None,
        }
    }
    guard.as_mut()?.choice(question, context, options).ok()
}

// --------------------------------------------------------------- relevance gate

const GATE_FILE: &str = "openjev-gate-int8.onnx";
const GATE_TOKENIZER_FILE: &str = "openjev-gate-tokenizer.json";
const GATE_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-gate-int8.onnx";
const GATE_TOKENIZER_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-gate-tokenizer.json";
const GATE_SHA256: &str = "5b3b0bbbd49f7f1564d42c4d869dcb0304cd4e9062ea1f8bfdc38d8c1fe12d3d";
const GATE_TOKENIZER_SHA256: &str = "79ea220416a57ca8acdf069d71875d5d2ddadef66021aeb6c120a7c546c04344";

/// A small fine-tuned encoder that scores relevance: (addressed, response).
struct Gate {
    session: Session,
    tokenizer: Tokenizer,
}

impl Gate {
    fn load(assets: &Path) -> Result<Gate> {
        crate::audio::prepare_ort()?;
        let dir = dir(assets);
        let tokenizer = Tokenizer::from_file(dir.join(GATE_TOKENIZER_FILE))
            .map_err(|e| anyhow!("gate tokenizer: {e}"))?;
        let session = Session::builder()?
            .commit_from_file(dir.join(GATE_FILE))
            .context("Could not load the relevance model")?;
        Ok(Gate { session, tokenizer })
    }

    fn score(&mut self, input: &str) -> Result<(f32, f32)> {
        let encoded = self
            .tokenizer
            .encode(input, true)
            .map_err(|e| anyhow!("gate tokenizer: {e}"))?;
        let len = encoded.get_ids().len();
        let ids: Vec<i64> = encoded.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoded.get_attention_mask().iter().map(|&x| x as i64).collect();
        let ids = Array2::from_shape_vec((1, len), ids)?;
        let mask = Array2::from_shape_vec((1, len), mask)?;
        let outputs = self.session.run(ort::inputs![
            "input_ids" => Value::from_array(ids)?,
            "attention_mask" => Value::from_array(mask)?,
        ])?;
        let (_shape, logits) = outputs[0].try_extract_tensor::<f32>()?;
        let sigmoid = |x: f32| 1.0 / (1.0 + (-x).exp());
        let addressed = logits.first().copied().context("gate output missing")?;
        let response = logits.get(1).copied().context("gate output missing")?;
        Ok((sigmoid(addressed), sigmoid(response)))
    }
}

fn gate_assets() -> Option<PathBuf> {
    static ASSETS: OnceLock<Option<PathBuf>> = OnceLock::new();
    ASSETS
        .get_or_init(|| {
            let assets = crate::config::Settings::load().ok()?.assets().ok()?;
            let dir = dir(&assets);
            (dir.join(GATE_FILE).is_file() && dir.join(GATE_TOKENIZER_FILE).is_file())
                .then_some(assets)
        })
        .clone()
}

/// True when the fine-tuned relevance model is installed.
pub fn gate_available() -> bool {
    gate_assets().is_some()
}

/// Score an utterance for relevance. The caller formats the input and applies
/// the accept rule; returns `(addressed, response)` probabilities.
pub fn gate(input: &str) -> Option<(f32, f32)> {
    static GATE: OnceLock<Mutex<Option<Gate>>> = OnceLock::new();
    let assets = gate_assets()?;
    let cell = GATE.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().ok()?;
    if guard.is_none() {
        match Gate::load(&assets) {
            Ok(gate) => *guard = Some(gate),
            Err(_) => return None,
        }
    }
    guard.as_mut()?.score(input).ok()
}

/// Download the gate model too, as part of `acc decider install`.
pub async fn download_gate(assets: &Path) -> Result<()> {
    let dir = dir(assets);
    std::fs::create_dir_all(&dir)?;
    crate::stt_models::fetch(GATE_URL, &dir.join(GATE_FILE), GATE_SHA256).await?;
    crate::stt_models::fetch(GATE_TOKENIZER_URL, &dir.join(GATE_TOKENIZER_FILE), GATE_TOKENIZER_SHA256)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs only when the model is installed; skips silently otherwise so the
    /// offline suite stays green without the 150 MB asset.
    #[test]
    fn routes_a_coding_request_when_installed() {
        let Ok(assets) = crate::config::Settings::load().and_then(|s| s.assets()) else {
            return;
        };
        if !installed(&assets) {
            return;
        }
        if let Err(e) = Decider::load(&assets) {
            panic!("decider load failed: {e:#}");
        }
        let options = [
            "General conversation, a quick question, or a local control",
            "A software engineering task: code, debug, git, tests",
            "Email, calendar, docs, Slack, or another connected app",
        ];
        let choice = choice(
            "What kind of request is this?",
            "fix the failing test in parser.py",
            &options,
        )
        .expect("local decision");
        assert_eq!(choice.index, 1, "conf={} abstain={}", choice.confidence, choice.abstain);
    }

    /// Runs only when the gate model is installed; skips otherwise.
    #[test]
    fn gate_separates_requests_from_background_when_installed() {
        let Ok(assets) = crate::config::Settings::load().and_then(|s| s.assets()) else {
            return;
        };
        if !installed(&assets) || !gate_available() {
            return;
        }
        let request = gate(
            "wake_addressed: no\nconversation:\n(none)\nutterance: what's the weather in college station tomorrow",
        )
        .expect("gate");
        let background = gate(
            "wake_addressed: no\nconversation:\n(none)\nutterance: and then she told him to wait outside",
        )
        .expect("gate");
        assert!(
            request.0 >= 0.5 && request.1 >= 0.5,
            "request should be accepted: {request:?}"
        );
        assert!(
            background.0 < 0.5 && background.1 < 0.5,
            "background should be rejected: {background:?}"
        );
    }
}
