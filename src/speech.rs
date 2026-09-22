use crate::config::{self, Tts};
use anyhow::{ensure, Context, Result};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub fn spoken_text(markdown: &str) -> String {
    let mut result = String::new();
    let mut code = false;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => {
                code = false;
                result.push_str(" Code is shown in the terminal. ");
            }
            Event::Text(t) | Event::Code(t) if !code => result.push_str(&t),
            Event::SoftBreak
            | Event::HardBreak
            | Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Item) => result.push(' '),
            _ => {}
        }
    }
    result
        .replace('|', ", ")
        .replace(['*', '`', '#', '_', '[', ']', '{', '}', '<', '>'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(6000)
        .collect()
}

pub fn speak_chunks(text: &str) -> Vec<String> {
    let text = spoken_text(text);
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut buf = String::new();
    for word in text.split_whitespace() {
        let trial = if buf.is_empty() {
            word.to_string()
        } else {
            format!("{buf} {word}")
        };
        let boundary = word.ends_with('.') || word.ends_with('?') || word.ends_with('!');
        if trial.chars().count() > 220 && !buf.is_empty() {
            out.push(std::mem::take(&mut buf));
            buf = word.to_string();
        } else {
            buf = trial;
        }
        if boundary && buf.chars().count() >= 24 {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}
pub fn request(text: &str, settings: &Tts) -> Value {
    json!({"model_id":settings.model,"transcript":text,"voice":{"mode":"id","id":settings.voice},"language":"en","generation_config":{"speed":settings.speed.clamp(0.6,1.5)},"output_format":{"container":"raw","encoding":"pcm_s16le","sample_rate":16000}})
}
pub async fn synthesize(text: &str, settings: &Tts) -> Result<Vec<u8>> {
    Ok(synthesize_measured(text, settings).await?.0)
}
async fn synthesize_measured(text: &str, settings: &Tts) -> Result<(Vec<u8>, f64)> {
    ensure!(
        !settings.voice.is_empty(),
        "Choose a Cartesia voice with acc tts setup"
    );
    let key = config::secret("cartesia", "CARTESIA_API_KEY")?;
    let began = std::time::Instant::now();
    let mut first_byte_ms = None;
    let mut response = crate::http::client()?
        .post("https://api.cartesia.ai/tts/bytes")
        .bearer_auth(key)
        .header("Cartesia-Version", "2026-08-14")
        .json(&request(text, settings))
        .send()
        .await?;
    ensure!(response.status().is_success(),"Cartesia returned HTTP {}. Check your key, voice, model, and account credit using acc tts setup.",response.status());
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if !chunk.is_empty() && first_byte_ms.is_none() {
            first_byte_ms = Some(began.elapsed().as_secs_f64() * 1000.0);
        }
        ensure!(
            bytes.len() + chunk.len() <= 32 * 1024 * 1024,
            "Cartesia audio exceeded the 32 MiB limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    let wav = if bytes.starts_with(b"RIFF") {
        bytes
    } else {
        crate::audio::wrap_pcm16_mono(&bytes, 16_000)?
    };
    Ok((
        wav,
        first_byte_ms.unwrap_or(began.elapsed().as_secs_f64() * 1000.0),
    ))
}

#[derive(Default)]
struct PcmDecoder {
    pending: Option<u8>,
}
impl PcmDecoder {
    fn decode(&mut self, bytes: &[u8]) -> Vec<f32> {
        let mut samples = Vec::with_capacity(bytes.len().div_ceil(2));
        for &byte in bytes {
            if let Some(low) = self.pending.take() {
                samples.push(i16::from_le_bytes([low, byte]) as f32 / 32768.0);
            } else {
                self.pending = Some(byte);
            }
        }
        samples
    }
}

async fn stream_cartesia(
    text: &str,
    settings: &Tts,
    capture: Arc<crate::audio::SpeechState>,
    playing: Arc<AtomicBool>,
) -> Result<()> {
    let key = config::secret("cartesia", "CARTESIA_API_KEY")?;
    let began = std::time::Instant::now();
    let mut response = crate::http::client()?
        .post("https://api.cartesia.ai/tts/bytes")
        .bearer_auth(key)
        .header("Cartesia-Version", "2026-08-14")
        .json(&request(text, settings))
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "Cartesia returned HTTP {}",
        response.status()
    );
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let player = crate::stream_playback::Player::start(receiver, capture, playing, settings.volume);
    let mut decoder = PcmDecoder::default();
    let mut received = 0;
    while let Some(bytes) = response.chunk().await? {
        if bytes.is_empty() {
            continue;
        }
        if received == 0 {
            crate::usage::record_latency("TTS first audio received", began.elapsed());
        }
        received += bytes.len();
        ensure!(
            received <= 32 * 1024 * 1024,
            "Cartesia audio exceeded 32 MiB"
        );
        for packet in decoder.decode(&bytes).chunks(1600) {
            sender
                .send(packet.to_vec())
                .await
                .context("Streaming speaker stopped")?;
        }
    }
    ensure!(
        decoder.pending.is_none() && received > 0,
        "Cartesia returned incomplete PCM audio"
    );
    crate::usage::record_tts("cartesia", text.chars().count());
    drop(sender);
    player.finish().await
}

pub async fn benchmark(text: &str, settings: &Tts, runs: usize) -> Result<Value> {
    let mut rows = Vec::new();
    for _ in 0..runs {
        let start = std::time::Instant::now();
        let (wav, first_byte_ms) = if settings.provider == "cartesia" {
            let (wav, ms) = synthesize_measured(text, settings).await?;
            (wav, Some(ms))
        } else {
            (render_uncached(text, settings).await?, None)
        };
        let total_ms = start.elapsed().as_secs_f64() * 1000.0;
        let (rate, samples) = crate::audio::decode_wav(&wav)?;
        let seconds = samples.len() as f64 / rate as f64;
        crate::usage::record_tts(&settings.provider, text.chars().count());
        rows.push(serde_json::json!({"total_ms":total_ms,"first_byte_ms":first_byte_ms,"audio_seconds":seconds,"real_time_factor":total_ms/(seconds*1000.0)}));
    }
    Ok(
        serde_json::json!({"provider":settings.provider,"runs":rows,"note":"No disk/memory audio cache, no playback. First run includes connection/worker startup. First byte is network arrival, not audible playback."}),
    )
}

pub async fn benchmark_playback(text: &str, settings: &Tts, runs: usize) -> Result<Value> {
    let mut rows = Vec::new();
    for _ in 0..runs {
        clear_memory_cache();
        let start = std::time::Instant::now();
        let mut job = self::start(
            text.into(),
            settings.clone(),
            Arc::new(crate::audio::SpeechState::default()),
        );
        let mut playback_ms = None;
        while !job.task.is_finished() {
            if playback_ms.is_none() && job.is_playing() {
                playback_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        (&mut job.task).await??;
        rows.push(serde_json::json!({"playback_started_ms":playback_ms,"complete_ms":start.elapsed().as_secs_f64()*1000.0}));
    }
    Ok(
        serde_json::json!({"provider":settings.provider,"streaming":settings.streaming,"runs":rows,
        "note":"Plays synthetic test text. Software playback flag timing, not an acoustic measurement; includes device/connection startup. Audio cache cleared each run."}),
    )
}
pub async fn cartesia_stt(samples: &[f32]) -> Result<String> {
    let key = config::secret("cartesia", "CARTESIA_API_KEY")?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for sample in samples {
            writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)?;
        }
        writer.finalize()?;
    }
    let wav = cursor.into_inner();
    let client = crate::http::client()?;
    for model in ["ink-2", "ink-whisper"] {
        let part = reqwest::multipart::Part::bytes(wav.clone())
            .file_name("speech.wav")
            .mime_str("audio/wav")?;
        let form = reqwest::multipart::Form::new()
            .text("model", model)
            .text("language", "en")
            .part("file", part);
        let response = client
            .post("https://api.cartesia.ai/stt")
            .timeout(Duration::from_secs(8))
            .bearer_auth(&key)
            .header("Cartesia-Version", "2026-08-14")
            .multipart(form)
            .send()
            .await?;
        if !response.status().is_success() {
            continue;
        }
        let data: Value = response.json().await?;
        if let Some(text) = data["text"]
            .as_str()
            .or_else(|| data["transcript"].as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Ok(text.trim().to_owned());
        }
    }
    anyhow::bail!("Cartesia Ink STT returned no transcript")
}
/// A selectable Cartesia voice. Cached locally so the settings picker works
/// offline after the list has been fetched once.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CartesiaVoice {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Cartesia's built-in default voice (Classy British Man).
pub const DEFAULT_CARTESIA_VOICE: &str = "95856005-0332-41b0-935f-352e296aa0df";

fn builtin_voices() -> Vec<CartesiaVoice> {
    vec![CartesiaVoice {
        id: DEFAULT_CARTESIA_VOICE.into(),
        name: "Classy British Man".into(),
        description: "Measured, polished British male; Accessor's default".into(),
    }]
}

/// Voices fetched from Cartesia and cached under the settings directory.
pub fn cached_voices() -> Vec<CartesiaVoice> {
    config::home()
        .ok()
        .and_then(|p| std::fs::read(p.join("voices.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// A short id for compact display, e.g. `95856005…`.
pub fn short_id(id: &str) -> String {
    let head: String = id.chars().take(8).collect();
    if id.chars().count() > 8 {
        format!("{head}…")
    } else {
        head
    }
}

/// The full list used by pickers: cached account voices plus the built-in
/// default, de-duplicated by id.
pub fn voice_catalog() -> Vec<CartesiaVoice> {
    let mut list = cached_voices();
    if list.is_empty() {
        list = builtin_voices();
    }
    for voice in builtin_voices() {
        if !list.iter().any(|v| v.id == voice.id) {
            list.push(voice);
        }
    }
    list
}

/// A catalog that always includes `current`, even when it is a private voice
/// that never appeared in the fetched list.
pub fn catalog_with_current(current: &str) -> Vec<CartesiaVoice> {
    let mut list = voice_catalog();
    if !current.is_empty() && !list.iter().any(|v| v.id == current) {
        list.insert(
            0,
            CartesiaVoice {
                id: current.into(),
                name: "Custom voice".into(),
                description: String::new(),
            },
        );
    }
    list
}

/// Friendly name for a voice id, falling back to the id itself.
pub fn voice_name(id: &str) -> String {
    voice_catalog()
        .into_iter()
        .find(|v| v.id == id)
        .map(|v| v.name)
        .unwrap_or_else(|| id.to_owned())
}

fn normalized(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Resolve typed input to a Cartesia voice id. Accepts an exact id, a unique
/// friendly name (or its last word), `default`, or a plausible raw id.
pub fn resolve_voice(value: &str, catalog: &[CartesiaVoice]) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.eq_ignore_ascii_case("default") {
        return Some(DEFAULT_CARTESIA_VOICE.into());
    }
    if let Some(voice) = catalog.iter().find(|v| v.id.eq_ignore_ascii_case(value)) {
        return Some(voice.id.clone());
    }
    let needle = normalized(value);
    let unique = |ok: Vec<&CartesiaVoice>| {
        if ok.len() == 1 {
            Some(ok[0].id.clone())
        } else {
            None
        }
    };
    if let Some(id) = unique(
        catalog
            .iter()
            .filter(|v| normalized(&v.name) == needle)
            .collect(),
    ) {
        return Some(id);
    }
    if let Some(id) = unique(
        catalog
            .iter()
            .filter(|v| {
                v.name
                    .split_whitespace()
                    .last()
                    .is_some_and(|w| normalized(w) == needle)
            })
            .collect(),
    ) {
        return Some(id);
    }
    // A private voice id the user pasted directly. Require an id-like shape
    // (a hyphen or a long token) so a misspelled name cannot silently become a
    // broken setting.
    if value.len() <= 128
        && !value.contains(char::is_whitespace)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
        && (value.contains('-') || value.len() >= 16)
    {
        return Some(value.to_owned());
    }
    None
}

fn parse_voices(data: &Value) -> Vec<CartesiaVoice> {
    data["data"]
        .as_array()
        .or_else(|| data.as_array())
        .map(|voices| {
            voices
                .iter()
                .filter_map(|v| {
                    let id = v["id"].as_str()?;
                    Some(CartesiaVoice {
                        id: id.into(),
                        name: v["name"]
                            .as_str()
                            .filter(|n| !n.trim().is_empty())
                            .unwrap_or(id)
                            .into(),
                        description: v["description"].as_str().unwrap_or("").trim().into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fetch the account's voices from Cartesia and cache them locally.
pub async fn fetch_voices() -> Result<Vec<CartesiaVoice>> {
    let key = config::secret("cartesia", "CARTESIA_API_KEY")?;
    let response = crate::http::client()?
        .get("https://api.cartesia.ai/voices")
        .query(&[("limit", "100")])
        .header("Cartesia-Version", "2026-08-14")
        .bearer_auth(key)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "Cartesia voices returned HTTP {}. Check the API key in /tts key.",
        response.status()
    );
    let data: Value = response.json().await?;
    let voices = parse_voices(&data);
    ensure!(!voices.is_empty(), "Cartesia returned no voices");
    config::save_private(
        &config::home()?.join("voices.json"),
        &serde_json::to_vec(&voices)?,
    )?;
    Ok(voices)
}

/// Print the account's Cartesia voices, optionally filtered by a search term,
/// marking the currently selected voice.
pub async fn voices(search: Option<&str>) -> Result<()> {
    let voices = fetch_voices().await?;
    let current = config::Settings::load()
        .map(|s| s.tts.voice)
        .unwrap_or_default();
    let needle = search.unwrap_or("").trim().to_lowercase();
    let mut shown = 0;
    for v in &voices {
        if !needle.is_empty()
            && !v.name.to_lowercase().contains(&needle)
            && !v.id.to_lowercase().contains(&needle)
            && !v.description.to_lowercase().contains(&needle)
        {
            continue;
        }
        shown += 1;
        let marker = if v.id == current { "  ← current" } else { "" };
        let description = if v.description.is_empty() {
            String::new()
        } else {
            format!(" — {}", crate::ui::safe(&v.description))
        };
        println!(
            "{}  {}  ({}){}{}",
            shown,
            crate::ui::safe(&v.name),
            v.id,
            description,
            marker
        );
    }
    if shown == 0 {
        println!(
            "No Cartesia voices matched \"{}\".",
            crate::ui::safe(search.unwrap_or(""))
        );
    }
    Ok(())
}
pub struct Job {
    pub task: tokio::task::JoinHandle<Result<()>>,
    stop: Arc<AtomicBool>,
    playing: Arc<AtomicBool>,
}
impl Job {
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.task.abort();
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}
// JoinHandle drop alone detaches work. Keep synthesis tied to the speech job so
// lock/cancel also stops in-flight HTTP requests and local synthesis workers.
struct Synthesis(tokio::task::JoinHandle<Result<Vec<u8>>>);
impl Synthesis {
    fn start(text: String, settings: Tts) -> Self {
        Self(tokio::spawn(async move { render(&text, &settings).await }))
    }
    async fn finish(mut self) -> Result<Vec<u8>> {
        (&mut self.0).await?
    }
}
impl Drop for Synthesis {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub fn start(text: String, settings: Tts, capture: Arc<crate::audio::SpeechState>) -> Job {
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let pause = paused.clone();
    let flag = stop.clone();
    let playing = Arc::new(AtomicBool::new(false));
    let playback = playing.clone();
    let task = tokio::spawn(async move {
        let parts = speak_chunks(&text);
        if parts.is_empty() || settings.provider == "off" {
            return Ok(());
        }
        if settings.provider == "cartesia" && settings.streaming {
            for part in parts {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                stream_cartesia(&part, &settings, capture.clone(), playback.clone()).await?;
            }
            return Ok(());
        }
        let mut upcoming = {
            let part = parts[0].clone();
            let cfg = settings.clone();
            Some(Synthesis::start(part, cfg))
        };
        for i in 0..parts.len() {
            if flag.load(Ordering::SeqCst) {
                break;
            }
            let wav = upcoming.take().unwrap().finish().await?;
            if i + 1 < parts.len() {
                let next = parts[i + 1].clone();
                let cfg = settings.clone();
                upcoming = Some(Synthesis::start(next, cfg));
            }
            // Synthesis may finish after the user has started another phrase.
            // Wait through capture, recognition and relevance checking before playback.
            while capture.holding() {
                if flag.load(Ordering::SeqCst) {
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            let capture = capture.clone();
            let pause = pause.clone();
            let flag = flag.clone();
            let playback = playback.clone();
            tokio::task::spawn_blocking(move || {
                capture.playback(true);
                playback.store(true, Ordering::SeqCst);
                let result = crate::audio::play_wav(&wav, &flag, pause, settings.volume);
                playback.store(false, Ordering::SeqCst);
                capture.playback(false);
                result
            })
            .await??;
        }
        Ok(())
    });
    Job {
        task,
        stop,
        playing,
    }
}
pub fn local_python(s: &config::Settings) -> Result<std::path::PathBuf> {
    Ok(s.assets()?.join("runtime/kokoro").join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    }))
}
struct Local {
    _child: tokio::process::Child,
    input: tokio::process::ChildStdin,
    output: tokio::io::BufReader<tokio::process::ChildStdout>,
    assets: std::path::PathBuf,
}
static LOCAL: std::sync::OnceLock<tokio::sync::Mutex<Option<Local>>> = std::sync::OnceLock::new();
async fn local_request(request: Value) -> Result<(Value, Vec<u8>)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let settings = config::Settings::load()?;
    let assets = settings.assets()?;
    let mut slot = LOCAL
        .get_or_init(|| tokio::sync::Mutex::new(None))
        .lock()
        .await;
    // Taking ownership makes cancellation drop/kill the worker instead of leaving
    // a partially consumed response for the next request.
    let mut worker = match slot.take().filter(|w| w.assets == assets) {
        Some(w) => w,
        None => {
            let python = local_python(&settings)?;
            ensure!(
                python.is_file(),
                "Kokoro is not installed. Run python scripts/setup_tts.py first."
            );
            let mut child = tokio::process::Command::new(python)
                .arg(assets.join("runtime/kokoro/worker.py"))
                .arg(assets.join("models/kokoro"))
                .env("PYTHONUTF8", "1")
                .env("OMP_NUM_THREADS", "2")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            Local {
                input: child.stdin.take().context("Kokoro stdin missing")?,
                output: tokio::io::BufReader::new(
                    child.stdout.take().context("Kokoro stdout missing")?,
                ),
                _child: child,
                assets,
            }
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        worker
            .input
            .write_all(format!("{request}\n").as_bytes())
            .await?;
        let mut header = String::new();
        let n = worker.output.read_line(&mut header).await?;
        ensure!(
            n > 0 && n < 64 * 1024,
            "Kokoro worker stopped. Re-run setup and check Python/platform compatibility."
        );
        let header: Value = serde_json::from_str(&header)?;
        ensure!(
            header["ok"] == true,
            "Kokoro: {}",
            header["error"].as_str().unwrap_or("synthesis failed")
        );
        let size = header["bytes"].as_u64().context("Missing audio size")?;
        ensure!(size <= 32 * 1024 * 1024, "Kokoro audio exceeded 32 MiB");
        let mut bytes = vec![0; size as usize];
        worker.output.read_exact(&mut bytes).await?;
        Ok((header, bytes))
    })
    .await
    .context("Kokoro synthesis timed out")?;
    if result.is_ok() {
        *slot = Some(worker);
    }
    result
}
fn tts_cache_key(text: &str, settings: &Tts) -> Option<String> {
    if text.chars().count() > 240 || settings.provider == "off" {
        return None;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    settings.provider.hash(&mut hasher);
    settings.voice.hash(&mut hasher);
    settings.local_voice.hash(&mut hasher);
    settings.model.hash(&mut hasher);
    settings.speed.to_bits().hash(&mut hasher);
    text.hash(&mut hasher);
    Some(format!("{:016x}.wav", hasher.finish()))
}
type AudioCache = std::collections::VecDeque<(String, Vec<u8>)>;
static AUDIO_CACHE: std::sync::Mutex<AudioCache> =
    std::sync::Mutex::new(std::collections::VecDeque::new());
static CACHE_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub fn clear_memory_cache() {
    if let Ok(mut cache) = AUDIO_CACHE.lock() {
        CACHE_EPOCH.fetch_add(1, Ordering::SeqCst);
        cache.clear();
    }
}
pub fn clear_disk_cache() -> Result<usize> {
    let directory = config::home()?.join("tts-cache");
    let mut removed = 0;
    if directory.is_dir() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            // Only the old generated hexadecimal WAV names, never arbitrary user files.
            let path = entry.path();
            if entry.file_type()?.is_file()
                && path.extension().is_some_and(|v| v == "wav")
                && path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.len() == 16 && v.chars().all(|c| c.is_ascii_hexdigit()))
            {
                std::fs::remove_file(path)?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}
fn tts_cache_get(text: &str, settings: &Tts) -> Option<Vec<u8>> {
    let name = tts_cache_key(text, settings)?;
    AUDIO_CACHE
        .lock()
        .ok()?
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, bytes)| bytes.clone())
}
fn tts_cache_put(text: &str, settings: &Tts, bytes: &[u8], epoch: u64) {
    crate::usage::record_tts(&settings.provider, text.chars().count());
    let Some(name) = tts_cache_key(text, settings) else {
        return;
    };
    const MAX_BYTES: usize = 8 * 1024 * 1024;
    if bytes.len() > MAX_BYTES {
        return;
    }
    if let Ok(mut cache) = AUDIO_CACHE.lock() {
        if epoch != CACHE_EPOCH.load(Ordering::SeqCst) {
            return;
        }
        cache.retain(|(key, _)| *key != name);
        while cache.len() >= 32
            || cache.iter().map(|(_, v)| v.len()).sum::<usize>() + bytes.len() > MAX_BYTES
        {
            cache.pop_front();
        }
        cache.push_back((name, bytes.to_vec()));
    }
}
pub async fn render(text: &str, settings: &Tts) -> Result<Vec<u8>> {
    let epoch = CACHE_EPOCH.load(Ordering::SeqCst);
    if let Some(bytes) = tts_cache_get(text, settings) {
        crate::usage::record_diagnostic("TTS cache hit");
        return Ok(bytes);
    }
    let began = std::time::Instant::now();
    let bytes = render_uncached(text, settings).await?;
    crate::usage::record_latency("Speech synthesis", began.elapsed());
    tts_cache_put(text, settings, &bytes, epoch);
    Ok(bytes)
}
async fn render_uncached(text: &str, settings: &Tts) -> Result<Vec<u8>> {
    if settings.provider == "system" {
        crate::audio::synthesize_system(text, settings.speed).await
    } else if settings.provider == "kokoro" {
        Ok(local_request(
            json!({"text":text,"voice":settings.local_voice,"speed":settings.speed.clamp(0.6,1.5)}),
        )
        .await?
        .1)
    } else {
        synthesize(text, settings).await
    }
}
pub async fn local_voices(_settings: &config::Settings) -> Result<()> {
    let (header, _) = local_request(json!({"action":"voices"})).await?;
    if let Some(voices) = header["voices"].as_array() {
        for voice in voices {
            println!("{}", crate::ui::safe(voice.as_str().unwrap_or("")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_packets_can_split_samples_at_any_byte() {
        let mut decoder = PcmDecoder::default();
        assert!(decoder.decode(&[0]).is_empty());
        assert_eq!(decoder.decode(&[64, 0, 192]), vec![0.5, -0.5]);
        assert!(decoder.pending.is_none());
    }
    #[test]
    fn strip_markup_and_urls() {
        assert_eq!(
            spoken_text("**Hello** [world](https://secret.example/a)."),
            "Hello world."
        );
        assert_eq!(spoken_text("east | west *now*"), "east , west now");
        assert_eq!(speak_chunks("A. F.").len(), 1);
        let parts = speak_chunks(
            "The time is 2:41 AM. It is Saturday. The community office opens at nine.",
        );
        assert!(parts.len() >= 2);
        assert!(parts[0].contains("2:41"));
    }
    #[test]
    fn cartesia_payload_has_current_shape() {
        let v = request("hello", &Tts::default());
        assert!(v["voice"].is_object());
        assert_eq!(v["output_format"]["container"], "raw");
        assert_eq!(v["output_format"]["encoding"], "pcm_s16le");
        assert_eq!(v["generation_config"]["speed"], 1.0);
        assert!(v.get("api_key").is_none());
    }
    #[test]
    fn cartesia_voice_list_parses_names_and_descriptions() {
        let data = json!({"data":[
            {"id":"x","name":"Alpha","description":"calm and deep"},
            {"id":"y"}
        ]});
        let list = parse_voices(&data);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Alpha");
        assert_eq!(list[0].description, "calm and deep");
        assert_eq!(list[1].name, "y");
    }
    #[test]
    fn cartesia_voice_resolution_accepts_ids_names_and_defaults() {
        let catalog = vec![CartesiaVoice {
            id: "abc-123".into(),
            name: "Test Voice".into(),
            description: String::new(),
        }];
        assert_eq!(
            resolve_voice("test voice", &catalog).as_deref(),
            Some("abc-123")
        );
        assert_eq!(resolve_voice("Voice", &catalog).as_deref(), Some("abc-123"));
        assert_eq!(
            resolve_voice("ABC-123", &catalog).as_deref(),
            Some("abc-123")
        );
        assert_eq!(
            resolve_voice("default", &catalog).as_deref(),
            Some(DEFAULT_CARTESIA_VOICE)
        );
        assert_eq!(
            resolve_voice("123e4567-e89b-12d3-a456-426614174000", &catalog).as_deref(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
        assert!(resolve_voice("not a real voice", &catalog).is_none());
        assert!(resolve_voice("man", &catalog).is_none());
    }
    #[test]
    fn catalog_with_current_keeps_private_voice_selectable() {
        let list = catalog_with_current("private-custom-id");
        assert!(list.iter().any(|v| v.id == "private-custom-id"));
        assert!(list.iter().any(|v| v.id == DEFAULT_CARTESIA_VOICE));
    }
}
