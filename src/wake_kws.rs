//! Always-on wake-phrase detection with a small sherpa-onnx zipformer KWS model.
//!
//! This is an *additional* trigger beside the full Canary wake path. The 3M
//! keyword model is an order of magnitude cheaper than Canary and can fire
//! mid-utterance, so it wakes and barges in faster and more reliably for clear
//! speech. It is less robust at low signal-to-noise than Canary (which still
//! finalizes every request), so Canary recognition is never removed: a KWS hit
//! only opens listening or interrupts playback, exactly like a rolling Canary
//! wake probe. The audio fed here is the echo-cancelled 16 kHz capture.
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig};

/// Folder name installed under `assets/models`. A differently dated download is
/// discovered by prefix, so a model refresh does not need a code change.
pub const MODEL_DIR: &str = "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20";
const ARCHIVE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20.tar.bz2";
const ARCHIVE_SHA256: &str = "68447f4fbc67e70eee3a93961f36e81e98f47aef73ce7e7ca00885c6cd3616a6";
/// Keyword bonus; the value used by sherpa's own keyword-spotter example.
pub const DEFAULT_SCORE: f32 = 1.0;
/// Detection threshold. Higher is more conservative; sherpa's default is 0.25.
pub const DEFAULT_THRESHOLD: f32 = 0.25;

/// Wake settings shared with the capture thread. A change bumps the generation
/// so the spotter is rebuilt without restarting the microphone.
pub struct Control {
    assets: PathBuf,
    params: Mutex<Params>,
    generation: AtomicU64,
}

#[derive(Clone)]
pub struct Params {
    pub code: String,
    pub aliases: Vec<String>,
    pub threshold: f32,
    pub score: f32,
    pub enabled: bool,
}

impl Control {
    pub fn new(assets: PathBuf, params: Params) -> Self {
        Self {
            assets,
            params: Mutex::new(params),
            generation: AtomicU64::new(1),
        }
    }
    pub fn assets(&self) -> &Path {
        &self.assets
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    pub fn snapshot(&self) -> Params {
        self.params
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
    pub fn set(&self, params: Params) {
        if let Ok(mut slot) = self.params.lock() {
            *slot = params;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// One loaded keyword spotter and its streaming input. Not shared: the capture
/// thread owns it.
pub struct Spotter {
    kws: KeywordSpotter,
    stream: sherpa_onnx::OnlineStream,
}

impl Spotter {
    pub fn load(assets: &Path, params: &Params) -> Result<Self> {
        let dir = model_dir(assets).context("wake-word model is not installed")?;
        let files = Files::discover(&dir)?;
        let keywords = keywords(&params.code, &params.aliases, &dir)?;
        ensure!(!keywords.trim().is_empty(), "no usable wake pronunciations");
        let mut config = KeywordSpotterConfig::default();
        config.model_config.transducer.encoder = Some(files.encoder);
        config.model_config.transducer.decoder = Some(files.decoder);
        config.model_config.transducer.joiner = Some(files.joiner);
        config.model_config.tokens = Some(files.tokens);
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = 1;
        config.model_config.debug = false;
        config.keywords_buf = Some(keywords);
        config.keywords_score = params.score;
        config.keywords_threshold = params.threshold;
        let kws = KeywordSpotter::create(&config).context("keyword spotter rejected the model")?;
        let stream = kws.create_stream();
        Ok(Self { kws, stream })
    }

    /// Feed 16 kHz mono samples. Returns the matched keyword surface when the
    /// phrase completed in this feed.
    pub fn feed(&mut self, samples: &[f32]) -> Option<String> {
        self.stream.accept_waveform(16_000, samples);
        let mut hit = None;
        while self.kws.is_ready(&self.stream) {
            self.kws.decode(&self.stream);
            if let Some(result) = self.kws.get_result(&self.stream) {
                if !result.keyword.is_empty() {
                    hit = Some(result.keyword);
                    self.kws.reset(&self.stream);
                }
            }
        }
        hit
    }
}

struct Files {
    encoder: String,
    decoder: String,
    joiner: String,
    tokens: String,
}

impl Files {
    /// Prefer the chunk-16 configuration, falling back to any complete set.
    fn discover(dir: &Path) -> Result<Self> {
        let names: Vec<String> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        let mut stems: Vec<String> = names
            .iter()
            .filter_map(|n| n.strip_prefix("encoder-"))
            .filter_map(|n| n.strip_suffix(".onnx"))
            .filter_map(|n| n.strip_suffix(".int8"))
            .map(str::to_owned)
            .collect();
        stems.sort_by_key(|s| match () {
            _ if s.contains("chunk-16") => 0,
            _ if s.contains("chunk-8") => 1,
            _ => 2,
        });
        for stem in stems {
            let enc_int8 = format!("encoder-{stem}.int8.onnx");
            let enc = if names.contains(&enc_int8) {
                enc_int8
            } else {
                format!("encoder-{stem}.onnx")
            };
            let dec = format!("decoder-{stem}.onnx");
            let join_int8 = format!("joiner-{stem}.int8.onnx");
            let join = if names.contains(&join_int8) {
                join_int8
            } else {
                format!("joiner-{stem}.onnx")
            };
            if names.contains(&enc) && names.contains(&dec) && names.contains(&join) {
                return Ok(Self {
                    encoder: dir.join(enc).display().to_string(),
                    decoder: dir.join(dec).display().to_string(),
                    joiner: dir.join(join).display().to_string(),
                    tokens: dir.join("tokens.txt").display().to_string(),
                });
            }
        }
        bail!("no complete keyword model set in {}", dir.display())
    }
}

/// Build sherpa keyword lines ("PHONE PHONE @surface") for a numeric wake code
/// and any aliases, using the model's `en.phone` lexicon for pronunciations.
fn keywords(code: &str, aliases: &[String], dir: &Path) -> Result<String> {
    let mut forms = crate::wake::spoken_forms(code);
    forms.extend(aliases.iter().cloned());
    let lexicon = dir.join("en.phone");
    ensure!(
        lexicon.is_file(),
        "no pronunciation lexicon at {}",
        lexicon.display()
    );

    let mut needed: HashSet<String> = HashSet::new();
    for form in &forms {
        for word in words(form) {
            needed.insert(word.to_ascii_uppercase());
        }
    }
    let mut dictionary: HashMap<String, String> = HashMap::new();
    for line in BufReader::new(std::fs::File::open(&lexicon)?).lines() {
        let line = line?;
        let mut parts = line.split_whitespace();
        let Some(word) = parts.next() else { continue };
        if !needed.contains(word) {
            continue;
        }
        let phones: Vec<&str> = parts.collect();
        if !phones.is_empty() {
            dictionary
                .entry(word.to_string())
                .or_insert_with(|| phones.join(" "));
        }
    }

    let mut out = String::new();
    for form in &forms {
        let parts = words(form);
        if parts.is_empty() {
            continue;
        }
        let mut phones = Vec::with_capacity(parts.len());
        let mut covered = true;
        for word in &parts {
            match dictionary.get(&word.to_ascii_uppercase()) {
                Some(p) => phones.push(p.clone()),
                None => {
                    covered = false;
                    break;
                }
            }
        }
        if !covered {
            continue;
        }
        let surface = parts
            .iter()
            .map(|w| w.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("_");
        out.push_str(&phones.join(" "));
        out.push_str(" @");
        out.push_str(&surface);
        out.push('\n');
    }
    Ok(out)
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Locate the installed keyword model, matching the fixed folder or a dated
/// download with the same prefix.
pub fn model_dir(assets: &Path) -> Option<PathBuf> {
    let models = assets.join("models");
    let fixed = models.join(MODEL_DIR);
    if fixed.join("tokens.txt").is_file() {
        return Some(fixed);
    }
    for entry in std::fs::read_dir(&models).ok()?.flatten() {
        let path = entry.path();
        let matches = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("sherpa-onnx-kws"));
        if matches && path.join("tokens.txt").is_file() {
            return Some(path);
        }
    }
    None
}

pub fn available(assets: &Path) -> bool {
    model_dir(assets).is_some()
}

/// Download and unpack the keyword model when it is missing. Called only when
/// the wake engine is enabled, so users who keep the Canary path pay nothing.
pub async fn ensure_ready(assets: &Path) -> Result<()> {
    if available(assets) {
        return Ok(());
    }
    let archive = assets.join("downloads").join("sherpa-kws.tar.bz2");
    crate::stt_models::fetch(ARCHIVE_URL, &archive, ARCHIVE_SHA256).await?;
    let models = assets.join("models");
    std::fs::create_dir_all(&models)?;
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&models)
        .status()
        .context("Could not extract the wake-word model (need tar on PATH)")?;
    ensure!(status.success(), "Extracting the wake-word model failed");
    let _ = std::fs::remove_file(&archive);
    ensure!(
        available(assets),
        "wake-word archive did not contain the expected model folder"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_forms_cover_numeric_codes() {
        assert_eq!(spoken_forms_case("29"), vec!["two nine", "twenty nine"]);
        assert_eq!(spoken_forms_case("7"), vec!["seven"]);
        assert_eq!(spoken_forms_case("105"), vec!["one zero five"]);
    }

    fn spoken_forms_case(code: &str) -> Vec<String> {
        let forms = crate::wake::spoken_forms(code);
        forms
            .iter()
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect()
    }

    #[test]
    fn keywords_derive_from_lexicon() {
        let dir = std::env::temp_dir().join("acc-kws-test");
        let _ = std::fs::create_dir_all(&dir);
        let lexicon = dir.join("en.phone");
        std::fs::write(
            &lexicon,
            "TWO T UW1\nNINE N AY1 N\nTWENTY T W EH1 N T IY0\n",
        )
        .unwrap();
        let out = keywords("29", &[], &dir).unwrap();
        assert!(out.contains("T UW1 N AY1 N @two_nine"), "{out}");
        assert!(
            out.contains("T W EH1 N T IY0 N AY1 N @twenty_nine"),
            "{out}"
        );
        let _ = std::fs::remove_file(&lexicon);
    }

    #[test]
    #[ignore = "requires the downloaded wake-word model and the benchmark WAV"]
    fn detects_the_wake_phrase_with_the_real_model() {
        let assets = std::env::var_os("ACC_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap()).join(".config/accessor/assets")
            });
        assert!(
            available(&assets),
            "model not installed under {}",
            assets.display()
        );
        let params = Params {
            code: "29".into(),
            aliases: vec![],
            threshold: DEFAULT_THRESHOLD,
            score: DEFAULT_SCORE,
            enabled: true,
        };
        let mut spotter = Spotter::load(&assets, &params).unwrap();
        let samples =
            transcribe_rs::audio::read_wav_samples(Path::new("tests/fixtures/voice-benchmark.wav"))
                .unwrap();
        let mut hit = None;
        for frame in samples.chunks(256) {
            if let Some(keyword) = spotter.feed(frame) {
                hit = Some(keyword);
                break;
            }
        }
        assert!(hit.is_some(), "the wake phrase was not detected");
        eprintln!("detected {hit:?}");
    }
}
