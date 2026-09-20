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
    ensure!(
        !settings.voice.is_empty(),
        "Choose a Cartesia voice with acc tts setup"
    );
    let key = config::secret("cartesia", "CARTESIA_API_KEY")?;
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
        ensure!(
            bytes.len() + chunk.len() <= 32 * 1024 * 1024,
            "Cartesia audio exceeded the 32 MiB limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    if bytes.starts_with(b"RIFF") {
        Ok(bytes)
    } else {
        crate::audio::wrap_pcm16_mono(&bytes, 16_000)
    }
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
pub async fn voices() -> Result<()> {
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
        "Cartesia voices returned HTTP {}",
        response.status()
    );
    let data: Value = response.json().await?;
    let voices = data["data"]
        .as_array()
        .or_else(|| data.as_array())
        .context("Unexpected voice list format")?;
    for v in voices {
        println!(
            "{}  {}",
            v["id"].as_str().unwrap_or(""),
            crate::ui::safe(v["name"].as_str().unwrap_or(""))
        );
    }
    if data["has_more"] == true {
        println!("Showing the first 100 voices; additional voices are available in your Cartesia dashboard.");
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
        let mut upcoming = {
            let part = parts[0].clone();
            let cfg = settings.clone();
            Some(tokio::spawn(async move { render(&part, &cfg).await }))
        };
        for i in 0..parts.len() {
            if flag.load(Ordering::SeqCst) {
                break;
            }
            let wav = upcoming.take().unwrap().await??;
            if i + 1 < parts.len() {
                let next = parts[i + 1].clone();
                let cfg = settings.clone();
                upcoming = Some(tokio::spawn(async move { render(&next, &cfg).await }));
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
fn tts_cache_get(text: &str, settings: &Tts) -> Option<Vec<u8>> {
    let name = tts_cache_key(text, settings)?;
    let path = config::home().ok()?.join("tts-cache").join(name);
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 16 {
        return None;
    }
    Some(bytes)
}
fn tts_cache_put(text: &str, settings: &Tts, bytes: &[u8]) {
    crate::usage::record_tts(&settings.provider, text.chars().count());
    let Some(name) = tts_cache_key(text, settings) else {
        return;
    };
    let Ok(dir) = config::home().map(|h| h.join("tts-cache")) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(name), bytes);
    if let Ok(files) = std::fs::read_dir(&dir) {
        let mut entries: Vec<_> = files
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
            .collect();
        if entries.len() > 250 {
            entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
            for extra in entries.iter().take(entries.len().saturating_sub(200)) {
                let _ = std::fs::remove_file(extra.path());
            }
        }
    }
}
pub async fn render(text: &str, settings: &Tts) -> Result<Vec<u8>> {
    if let Some(bytes) = tts_cache_get(text, settings) {
        crate::usage::record_diagnostic("TTS cache hit");
        return Ok(bytes);
    }
    let began = std::time::Instant::now();
    let bytes = if settings.provider == "system" {
        crate::audio::synthesize_system(text, settings.speed).await?
    } else if settings.provider == "kokoro" {
        local_request(
            json!({"text":text,"voice":settings.local_voice,"speed":settings.speed.clamp(0.6,1.5)}),
        )
        .await?
        .1
    } else {
        synthesize(text, settings).await?
    };
    crate::usage::record_latency("Speech synthesis", began.elapsed());
    tts_cache_put(text, settings, &bytes);
    Ok(bytes)
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
}
