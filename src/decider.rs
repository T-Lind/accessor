//! Local decision models through ONNX Runtime.
//!
//! - the openJev 151M decision model (`choice`: request routing, mascot tone)
//! - a fine-tuned 66M relevance gate (`gate`: addressed/response)
//!
//! Both load lazily on first use and are freed by a small reaper after ten
//! minutes idle, so an idle Accessor does not hold hundreds of megabytes but an
//! active turn reuses a warm session instead of paying the load cost. Weights
//! are Apache-2.0 / distilled, quantized to int8, and fetched on first use.

use anyhow::{anyhow, Context, Result};
use ndarray::Array2;
use ort::session::Session;
use ort::value::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
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

const GATE_FILE: &str = "openjev-gate-int8.onnx";
const GATE_TOKENIZER_FILE: &str = "openjev-gate-tokenizer.json";
const GATE_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-gate-int8.onnx";
const GATE_TOKENIZER_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-gate-tokenizer.json";
const GATE_SHA256: &str = "5b3b0bbbd49f7f1564d42c4d869dcb0304cd4e9062ea1f8bfdc38d8c1fe12d3d";
const GATE_TOKENIZER_SHA256: &str =
    "79ea220416a57ca8acdf069d71875d5d2ddadef66021aeb6c120a7c546c04344";

const RANK_FILE: &str = "openjev-rank-int8.onnx";
const RANK_TOKENIZER_FILE: &str = "openjev-rank-tokenizer.json";
const RANK_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-rank-int8.onnx";
const RANK_TOKENIZER_URL: &str =
    "https://github.com/T-Lind/accessor/releases/download/openjev-v1/openjev-rank-tokenizer.json";
const RANK_SHA256: &str = "f914ad5da01d8f2724f757bf91ab0aec903f0a454e4dffff8aa92ccebfb84732";
const RANK_TOKENIZER_SHA256: &str =
    "8b79639ec74b46604e730f505186eaafb1006d2fd00f2c4930d168bb7f894680";

/// Free a model after this long without a decision. Long enough that an active
/// conversation never reloads, short enough that idle Accessor reclaims memory.
const IDLE_TTL: Duration = Duration::from_secs(600);
const REAP_INTERVAL: Duration = Duration::from_secs(30);

pub fn dir(assets: &Path) -> PathBuf {
    assets.join("decider")
}

pub fn installed(assets: &Path) -> bool {
    let dir = dir(assets);
    dir.join(MODEL_FILE).is_file() && dir.join(TOKENIZER_FILE).is_file()
}

fn gate_installed(assets: &Path) -> bool {
    let dir = dir(assets);
    dir.join(GATE_FILE).is_file() && dir.join(GATE_TOKENIZER_FILE).is_file()
}

pub fn size_label() -> String {
    crate::stt_models::size_label(248_121_157)
}

/// Download the router/tone model, then the gate, with checksum verification.
pub async fn download(assets: PathBuf) -> Result<String> {
    let dir = dir(&assets);
    std::fs::create_dir_all(&dir)?;
    crate::stt_models::fetch(MODEL_URL, &dir.join(MODEL_FILE), MODEL_SHA256).await?;
    crate::stt_models::fetch(TOKENIZER_URL, &dir.join(TOKENIZER_FILE), TOKENIZER_SHA256).await?;
    download_gate(&assets).await?;
    download_rank(&assets).await?;
    Ok("Installed the local decision model.".into())
}

/// Download the gate model, as part of `acc decider install`.
pub async fn download_gate(assets: &Path) -> Result<()> {
    let dir = dir(assets);
    std::fs::create_dir_all(&dir)?;
    crate::stt_models::fetch(GATE_URL, &dir.join(GATE_FILE), GATE_SHA256).await?;
    crate::stt_models::fetch(
        GATE_TOKENIZER_URL,
        &dir.join(GATE_TOKENIZER_FILE),
        GATE_TOKENIZER_SHA256,
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------------- pool

/// A lazily-loaded model that is dropped after an idle period.
struct Slot<T> {
    model: Mutex<Option<(T, Instant)>>,
}

impl<T: Send> Slot<T> {
    const fn new() -> Self {
        Slot {
            model: Mutex::new(None),
        }
    }

    /// Load on demand, run `f`, and mark the model recently used.
    fn with<R>(&self, load: impl FnOnce() -> Result<T>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut guard = self.model.lock().ok()?;
        if guard.is_none() {
            *guard = Some((load().ok()?, Instant::now()));
        }
        let (model, last_used) = guard.as_mut()?;
        *last_used = Instant::now();
        Some(f(model))
    }

    fn reap(&self) {
        if let Ok(mut guard) = self.model.lock() {
            if guard
                .as_ref()
                .is_some_and(|(_, last)| last.elapsed() >= IDLE_TTL)
            {
                *guard = None;
            }
        }
    }
}

/// Resolve a static slot and start one reaper thread that frees it when idle.
fn lazy_slot<T: Send + 'static>(cell: &'static OnceLock<Slot<T>>) -> &'static Slot<T> {
    cell.get_or_init(|| {
        std::thread::spawn(move || loop {
            std::thread::sleep(REAP_INTERVAL);
            if let Some(slot) = cell.get() {
                slot.reap();
            }
        });
        Slot::new()
    })
}

// ------------------------------------------------------------------- models

struct Decider {
    session: Session,
    tokenizer: Tokenizer,
}

/// Keep background decisions from consuming speech/UI CPU between requests.
fn decision_session() -> Result<ort::session::builder::SessionBuilder> {
    Ok(Session::builder()?
        .with_intra_threads(2)
        .map_err(ort::Error::<()>::from)?
        .with_inter_threads(1)
        .map_err(ort::Error::<()>::from)?
        .with_independent_thread_pool()
        .map_err(ort::Error::<()>::from)?
        .with_intra_op_spinning(false)
        .map_err(ort::Error::<()>::from)?
        .with_inter_op_spinning(false)
        .map_err(ort::Error::<()>::from)?)
}

pub struct Choice {
    pub index: usize,
    pub confidence: f32,
    pub abstain: f32,
}

impl Decider {
    fn load(assets: &Path) -> Result<Decider> {
        crate::audio::prepare_ort()?;
        let dir = dir(assets);
        let tokenizer = Tokenizer::from_file(dir.join(TOKENIZER_FILE))
            .map_err(|e| anyhow!("decision tokenizer: {e}"))?;
        let session = decision_session()?
            .commit_from_file(dir.join(MODEL_FILE))
            .context("Could not load the decision model")?;
        Ok(Decider { session, tokenizer })
    }

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
        let logits = run_single(&mut self.session, &self.tokenizer, &prompt)?;
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
        let session = decision_session()?
            .commit_from_file(dir.join(GATE_FILE))
            .context("Could not load the relevance model")?;
        Ok(Gate { session, tokenizer })
    }

    fn score(&mut self, input: &str) -> Result<(f32, f32)> {
        let logits = run_single(&mut self.session, &self.tokenizer, input)?;
        let sigmoid = |x: f32| 1.0 / (1.0 + (-x).exp());
        let addressed = logits.first().copied().context("gate output missing")?;
        let response = logits.get(1).copied().context("gate output missing")?;
        Ok((sigmoid(addressed), sigmoid(response)))
    }
}

/// Tokenize and run one sequence, returning the flattened logits.
fn run_single(session: &mut Session, tokenizer: &Tokenizer, text: &str) -> Result<Vec<f32>> {
    let encoded = tokenizer
        .encode(text, true)
        .map_err(|e| anyhow!("tokenizer: {e}"))?;
    let len = encoded.get_ids().len();
    let ids: Vec<i64> = encoded.get_ids().iter().map(|&x| x as i64).collect();
    let mask: Vec<i64> = encoded
        .get_attention_mask()
        .iter()
        .map(|&x| x as i64)
        .collect();
    let ids = Array2::from_shape_vec((1, len), ids)?;
    let mask = Array2::from_shape_vec((1, len), mask)?;
    let outputs = session.run(ort::inputs![
        "input_ids" => Value::from_array(ids)?,
        "attention_mask" => Value::from_array(mask)?,
    ])?;
    let (_shape, logits) = outputs[0].try_extract_tensor::<f32>()?;
    Ok(logits.to_vec())
}

/// Cross-encoder reranker: scores a document against a query.
struct Rank {
    session: Session,
    tokenizer: Tokenizer,
}

impl Rank {
    fn load(assets: &Path) -> Result<Rank> {
        crate::audio::prepare_ort()?;
        let dir = dir(assets);
        let tokenizer = Tokenizer::from_file(dir.join(RANK_TOKENIZER_FILE))
            .map_err(|e| anyhow!("rank tokenizer: {e}"))?;
        let session = decision_session()?
            .commit_from_file(dir.join(RANK_FILE))
            .context("Could not load the reranker")?;
        Ok(Rank { session, tokenizer })
    }

    fn score(&mut self, query: &str, doc: &str) -> Result<f32> {
        let encoded = self
            .tokenizer
            .encode((query, doc), true)
            .map_err(|e| anyhow!("rank tokenizer: {e}"))?;
        let len = encoded.get_ids().len();
        let ids: Vec<i64> = encoded.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoded
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let types: Vec<i64> = encoded.get_type_ids().iter().map(|&x| x as i64).collect();
        let ids = Array2::from_shape_vec((1, len), ids)?;
        let mask = Array2::from_shape_vec((1, len), mask)?;
        let types = Array2::from_shape_vec((1, len), types)?;
        let outputs = self.session.run(ort::inputs![
            "input_ids" => Value::from_array(ids)?,
            "attention_mask" => Value::from_array(mask)?,
            "token_type_ids" => Value::from_array(types)?,
        ])?;
        let (_shape, logits) = outputs[0].try_extract_tensor::<f32>()?;
        let raw = logits.first().copied().context("rank output missing")?;
        Ok(1.0 / (1.0 + (-raw).exp()))
    }
}

fn assets_with(check: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let assets = crate::config::Settings::load().ok()?.assets().ok()?;
    check(&assets).then_some(assets)
}

fn choice_slot() -> &'static Slot<Decider> {
    static CELL: OnceLock<Slot<Decider>> = OnceLock::new();
    lazy_slot(&CELL)
}

fn gate_slot() -> &'static Slot<Gate> {
    static CELL: OnceLock<Slot<Gate>> = OnceLock::new();
    lazy_slot(&CELL)
}

/// True when the router/tone model is installed and can load lazily.
pub fn available() -> bool {
    assets_with(installed).is_some()
}

/// True when the fine-tuned relevance gate is installed.
pub fn gate_available() -> bool {
    assets_with(gate_installed).is_some()
}

/// Run a `choice` decision, or `None` when the model is absent or fails.
pub fn choice(question: &str, context: &str, options: &[&str]) -> Option<Choice> {
    let assets = assets_with(installed)?;
    choice_slot().with(
        || Decider::load(&assets),
        |decider| decider.choice(question, context, options).ok(),
    )?
}

/// Score an utterance for relevance. The caller formats the input and applies
/// the accept rule; returns `(addressed, response)` probabilities.
pub fn gate(input: &str) -> Option<(f32, f32)> {
    let assets = assets_with(gate_installed)?;
    gate_slot().with(|| Gate::load(&assets), |gate| gate.score(input).ok())?
}

fn rank_installed(assets: &Path) -> bool {
    let dir = dir(assets);
    dir.join(RANK_FILE).is_file() && dir.join(RANK_TOKENIZER_FILE).is_file()
}

fn rank_slot() -> &'static Slot<Rank> {
    static CELL: OnceLock<Slot<Rank>> = OnceLock::new();
    lazy_slot(&CELL)
}

/// True when the local reranker is installed.
pub fn rank_available() -> bool {
    assets_with(rank_installed).is_some()
}

/// Score how relevant `doc` is to `query` (0..1), or `None` when absent.
pub fn rank(query: &str, doc: &str) -> Option<f32> {
    let assets = assets_with(rank_installed)?;
    rank_slot().with(|| Rank::load(&assets), |rank| rank.score(query, doc).ok())?
}

/// Download the reranker, as part of `acc decider install`.
pub async fn download_rank(assets: &Path) -> Result<()> {
    let dir = dir(assets);
    std::fs::create_dir_all(&dir)?;
    crate::stt_models::fetch(RANK_URL, &dir.join(RANK_FILE), RANK_SHA256).await?;
    crate::stt_models::fetch(
        RANK_TOKENIZER_URL,
        &dir.join(RANK_TOKENIZER_FILE),
        RANK_TOKENIZER_SHA256,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_a_coding_request_when_installed() {
        let Ok(assets) = crate::config::Settings::load().and_then(|s| s.assets()) else {
            return;
        };
        if !installed(&assets) {
            return;
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
        assert_eq!(
            choice.index, 1,
            "conf={} abstain={}",
            choice.confidence, choice.abstain
        );
    }

    #[test]
    fn gate_separates_requests_from_background_when_installed() {
        let Ok(assets) = crate::config::Settings::load().and_then(|s| s.assets()) else {
            return;
        };
        if !gate_installed(&assets) {
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
            "accepted: {request:?}"
        );
        assert!(
            background.0 < 0.5 && background.1 < 0.5,
            "rejected: {background:?}"
        );
    }

    #[test]
    fn reranker_prefers_the_matching_document_when_installed() {
        let Ok(assets) = crate::config::Settings::load().and_then(|s| s.assets()) else {
            return;
        };
        if !rank_installed(&assets) {
            return;
        }
        let relevant =
            rank("what is my wifi password", "the wifi password is hunter2").expect("rank");
        let distractor = rank(
            "what is my wifi password",
            "the neighbour's dog was barking again",
        )
        .expect("rank");
        assert!(
            relevant > distractor,
            "relevant {relevant} should beat {distractor}"
        );
    }
}
