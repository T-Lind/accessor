//! Music runs in a bounded worker, separate from speech synthesis and capture.
//! MPV owns decoding/output; Spotify Soloist owns its account and Connect session.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    Local,
    Spotify,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Status,
    Play,
    Ambience,
    Pause,
    Resume,
    Stop,
    Volume,
    Next,
    Previous,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default)]
    pub backend: Backend,
    pub action: Action,
    pub source: Option<String>,
    pub volume: Option<u8>,
    #[serde(default)]
    pub repeat: bool,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.volume.is_none_or(|v| v <= 100),
            "Music volume must be 0–100"
        );
        ensure!(
            self.source.as_ref().is_none_or(|s| s.len() <= 4096),
            "Music source is too long"
        );
        ensure!(
            self.action != Action::Volume || self.volume.is_some(),
            "Volume needs a percentage"
        );
        ensure!(
            matches!(self.action, Action::Play | Action::Ambience) || self.source.is_none(),
            "This action does not accept a source"
        );
        ensure!(
            matches!(
                self.action,
                Action::Play | Action::Ambience | Action::Volume
            ) || self.volume.is_none(),
            "This action does not accept a volume"
        );
        ensure!(
            !self.repeat || (self.backend == Backend::Local && self.action == Action::Play),
            "Repeat is for local file playback"
        );
        if self.backend == Backend::Spotify {
            ensure!(
                self.action == Action::Volume || self.volume.is_none(),
                "Set Spotify volume with the volume action"
            );
            ensure!(
                self.action != Action::Ambience,
                "Use local ambience, or play a Spotify playlist URI"
            );
            if let Some(source) = &self.source {
                spotify_uri(source)?;
            }
        } else {
            ensure!(
                !matches!(self.action, Action::Next | Action::Previous),
                "Next/previous require Spotify"
            );
            if self.action == Action::Play {
                ensure!(self.source.is_some(), "Play needs a local audio file");
            }
            if self.action == Action::Ambience {
                ensure!(
                    matches!(self.source.as_deref(), Some("brown" | "rain" | "white")),
                    "Ambience options: brown, rain, white"
                );
            }
        }
        Ok(())
    }
}
fn spotify_uri(source: &str) -> Result<()> {
    let parts: Vec<_> = source.split(':').collect();
    ensure!(
        parts.len() == 3
            && parts[0] == "spotify"
            && matches!(parts[1], "track" | "album" | "playlist" | "episode")
            && parts[2].len() == 22
            && parts[2].bytes().all(|b| b.is_ascii_alphanumeric()),
        "Use a Spotify track, album, playlist, or episode URI"
    );
    Ok(())
}
pub fn parse(text: &str) -> Result<Request> {
    let text = text.strip_prefix("/music").context("Use /music")?.trim();
    let (backend, text) = if let Some(t) = text.strip_prefix("spotify ") {
        (Backend::Spotify, t)
    } else {
        (Backend::Local, text)
    };
    let (action, rest) = text.split_once(' ').unwrap_or((text, ""));
    let action = match action {
        "" | "status" => Action::Status, "play" => Action::Play, "ambience" => Action::Ambience,
        "pause" => Action::Pause, "resume" => Action::Resume, "stop" => Action::Stop,
        "volume" => Action::Volume, "next" => Action::Next, "previous" => Action::Previous,
        _ => bail!("Use /music play FILE, ambience brown|rain|white, pause, resume, stop, volume 0–100, or spotify ACTION [URI]"),
    };
    let rest = rest.trim();
    let request = Request {
        backend,
        action,
        source: (!rest.is_empty() && action != Action::Volume).then(|| rest.to_owned()),
        volume: (action == Action::Volume)
            .then(|| rest.parse().context("Use /music volume 0–100"))
            .transpose()?,
        repeat: false,
    };
    request.validate()?;
    Ok(request)
}

struct Work {
    request: Request,
    reply: Option<oneshot::Sender<Value>>,
}
pub struct Service {
    tx: mpsc::Sender<Work>,
    task: tokio::task::JoinHandle<()>,
}
impl Service {
    pub fn start(
        input: mpsc::Sender<crate::Input>,
        capture: Arc<crate::audio::SpeechState>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<Work>(8);
        let task = tokio::spawn(async move {
            let mut player = Player::default();
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    work = rx.recv() => {
                        let Some(work) = work else { break; };
                        if work.reply.as_ref().is_some_and(|reply| reply.is_closed()) {continue;}
                        let result = tokio::time::timeout(Duration::from_secs(3), player.apply(&work.request)).await;
                        let value = match result {
                            Ok(Ok(v)) => v,
                            Ok(Err(e)) => {
                                if work.request.backend == Backend::Local && matches!(work.request.action, Action::Play | Action::Ambience) { player.local = None; }
                                json!({"error":format!("{e:#}")})
                            },
                            Err(_) => { player.local = None; json!({"error":"Music operation timed out; local player stopped. Check Spotify status before retrying a remote command."}) },
                        };
                        if let Some(reply) = work.reply { let _ = reply.send(value); }
                        else { let _ = input.try_send(crate::Input::Warning(format!("Music: {value}"))); }
                    }
                    _ = tick.tick() => {
                        if let Err(e) = player.duck(capture.is_playing()).await {
                            player.local = None;
                            let _ = input.try_send(crate::Input::Warning(format!("Music stopped: {e:#}")));
                        }
                    }
                }
            }
        });
        Self { tx, task }
    }
    pub fn request(&self, request: Request, reply: Option<oneshot::Sender<Value>>) -> Result<()> {
        if let Err(e) = request.validate() {
            if let Some(reply) = reply {
                let _ = reply.send(json!({"error":format!("{e:#}")}));
            }
            return Err(e);
        }
        if let Err(error) = self.tx.try_send(Work { request, reply }) {
            let work = error.into_inner();
            let message = "Music is busy; try again when the current operation finishes";
            if let Some(reply) = work.reply {
                let _ = reply.send(json!({"error":message}));
            }
            bail!(message);
        }
        Ok(())
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
struct Player {
    local: Option<Local>,
    volume: u8,
    ducked: bool,
}
struct Local {
    child: tokio::process::Child,
    dir: tempfile::TempDir,
    source: String,
}
impl Player {
    async fn apply(&mut self, request: &Request) -> Result<Value> {
        request.validate()?;
        if request.backend == Backend::Spotify {
            return spotify(request).await;
        }
        if let Some(local) = &mut self.local {
            if local.child.try_wait()?.is_some() {
                self.local = None;
            }
        }
        match request.action {
            Action::Status => {
                if self.local.is_none() {
                    return Ok(
                        json!({"backend":"local","state":"stopped","player":"mpv","setup":"Install mpv on PATH, or set ACC_MPV_BIN. Spotify uses a separately paired Soloist player."}),
                    );
                }
                Ok(
                    json!({"backend":"local","source":self.local.as_ref().unwrap().source,"paused":self.command(json!(["get_property","pause"])).await?,"idle":self.command(json!(["get_property","idle-active"])).await?,"volume":self.volume,"ducked":self.ducked}),
                )
            }
            Action::Stop => {
                self.local = None;
                Ok(json!({"backend":"local","state":"stopped"}))
            }
            Action::Play | Action::Ambience => {
                ensure!(
                    cfg!(unix),
                    "Local music playback currently requires Linux or macOS"
                );
                let dir = tempfile::Builder::new().prefix("acc-music-").tempdir()?;
                let path = if request.action == Action::Ambience {
                    let path = dir.path().join("ambience.wav");
                    let output = path.clone();
                    let kind = request.source.clone().unwrap();
                    tokio::task::spawn_blocking(move || render_ambience(&kind, &output, 30))
                        .await??;
                    path
                } else {
                    let path = std::fs::canonicalize(request.source.as_ref().unwrap())
                        .context("Cannot open local music file")?;
                    ensure!(path.is_file(), "Music source must be a local audio file");
                    ensure!(
                        path.extension()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| matches!(
                                s.to_ascii_lowercase().as_str(),
                                "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus"
                            )),
                        "Use a local MP3, WAV, FLAC, OGG, M4A, AAC, or Opus file"
                    );
                    path
                };
                self.local = None;
                self.volume = request.volume.unwrap_or(25);
                self.ducked = false;
                let mut command = tokio::process::Command::new(executable("mpv", "ACC_MPV_BIN"));
                command
                    .args([
                        "--no-config",
                        "--no-terminal",
                        "--no-video",
                        "--idle=yes",
                        "--ytdl=no",
                        "--input-default-bindings=no",
                    ])
                    .arg(format!(
                        "--input-ipc-server={}",
                        dir.path().join("player.sock").display()
                    ))
                    .arg(format!("--volume={}", self.volume))
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true);
                let child = command
                    .spawn()
                    .context("Install mpv, or set ACC_MPV_BIN to its executable")?;
                self.local = Some(Local {
                    child,
                    dir,
                    source: request.source.clone().unwrap(),
                });
                for _ in 0..80 {
                    if self
                        .command(json!(["get_property", "idle-active"]))
                        .await
                        .is_ok()
                    {
                        break;
                    }
                    if let Some(status) = self.local.as_mut().unwrap().child.try_wait()? {
                        self.local = None;
                        bail!("MPV exited before starting: {status}");
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                self.command(json!([
                    "set_property",
                    "loop-file",
                    if request.repeat || request.action == Action::Ambience {
                        "inf"
                    } else {
                        "no"
                    }
                ]))
                .await?;
                self.command(json!([
                    "loadfile",
                    path.to_str().context("Music path must be UTF-8")?,
                    "replace"
                ]))
                .await?;
                for _ in 0..30 {
                    if !self
                        .command(json!(["get_property", "idle-active"]))
                        .await?
                        .as_bool()
                        .unwrap_or(true)
                        && self
                            .command(json!(["get_property", "audio-out-params"]))
                            .await
                            .is_ok_and(|v| v.is_object())
                    {
                        return Ok(
                            json!({"backend":"local","state":"playing","source":request.source,"volume":self.volume,"repeat":request.repeat || request.action == Action::Ambience}),
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
                self.local = None;
                bail!("MPV could not begin audio playback; check the file and output device")
            }
            Action::Pause | Action::Resume => {
                self.command(json!([
                    "set_property",
                    "pause",
                    request.action == Action::Pause
                ]))
                .await?;
                Ok(json!({"backend":"local","paused":request.action == Action::Pause}))
            }
            Action::Volume => {
                let volume = request.volume.unwrap();
                self.command(json!([
                    "set_property",
                    "volume",
                    volume as f64 * if self.ducked { 0.2 } else { 1.0 }
                ]))
                .await?;
                self.volume = volume;
                Ok(json!({"backend":"local","volume":self.volume}))
            }
            _ => bail!("Unsupported local music action"),
        }
    }
    fn effective_volume(&self) -> f64 {
        self.volume as f64 * if self.ducked { 0.2 } else { 1.0 }
    }
    async fn duck(&mut self, playing: bool) -> Result<()> {
        if self.local.is_none() || self.ducked == playing {
            return Ok(());
        }
        self.ducked = playing;
        self.command(json!(["set_property", "volume", self.effective_volume()]))
            .await?;
        Ok(())
    }
    async fn command(&mut self, command: Value) -> Result<Value> {
        let local = self.local.as_mut().context("No local music is playing")?;
        #[cfg(unix)]
        {
            use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
            let mut stream =
                tokio::net::UnixStream::connect(local.dir.path().join("player.sock")).await?;
            stream
                .write_all(format!("{}\n", json!({"command":command,"request_id":1})).as_bytes())
                .await?;
            let mut reader = BufReader::new(stream);
            tokio::time::timeout(Duration::from_millis(700), async {
                loop {
                    let mut line = Vec::new();
                    (&mut reader)
                        .take(8193)
                        .read_until(b'\n', &mut line)
                        .await?;
                    ensure!(!line.is_empty() && line.len() <= 8192, "Invalid MPV reply");
                    let value: Value = serde_json::from_slice(&line)?;
                    if value["request_id"] == 1 {
                        ensure!(value["error"] == "success", "MPV: {}", value["error"]);
                        return Ok(value["data"].clone());
                    }
                }
            })
            .await
            .context("MPV did not reply in time")?
        }
        #[cfg(not(unix))]
        {
            let _ = (local, command);
            bail!("Local music IPC currently requires Linux or macOS");
        }
    }
}
async fn spotify(request: &Request) -> Result<Value> {
    let mut command = tokio::process::Command::new(executable("soloist", "ACC_SOLOIST_BIN"));
    command.arg("ctl").arg(match request.action {
        Action::Status => "status",
        Action::Play | Action::Resume => "play",
        Action::Pause | Action::Stop => "pause",
        Action::Next => "next",
        Action::Previous => "prev",
        Action::Volume => "volume",
        _ => bail!("Unsupported Spotify action"),
    });
    if let Some(source) = &request.source {
        command.arg(source);
    }
    if let Some(volume) = request.volume {
        ensure!(
            request.action == Action::Volume,
            "Set Spotify volume with the volume action"
        );
        command.arg(volume.to_string());
    }
    command.kill_on_drop(true);
    let output = command.output().await.context(
        "Install and pair Spotify Soloist with Premium, then start it with --ws 127.0.0.1:0",
    )?;
    ensure!(
        output.status.success(),
        "Soloist exited {}: {}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
            .chars()
            .take(2000)
            .collect::<String>(),
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(2000)
            .collect::<String>()
    );
    Ok(
        json!({"backend":"spotify","receipt":if request.action == Action::Status {"Soloist returned status"} else {"Soloist accepted the command"},"detail":String::from_utf8_lossy(&output.stdout).chars().take(4000).collect::<String>()}),
    )
}

fn executable(name: &str, environment: &str) -> std::ffi::OsString {
    if let Some(path) = std::env::var_os(environment) {
        return path;
    }
    if let Ok(assets) = crate::config::Settings::load().and_then(|s| s.assets()) {
        let path = assets.join("music").join("bin").join(name);
        if path.is_file() {
            return path.into_os_string();
        }
    }
    name.into()
}
pub fn render_ambience(kind: &str, output: &std::path::Path, seconds: u32) -> Result<()> {
    ensure!(
        matches!(kind, "brown" | "rain" | "white"),
        "Ambience options: brown, rain, white"
    );
    ensure!(
        (1..=300).contains(&seconds),
        "Ambience duration must be 1–300 seconds"
    );
    let rate = 44100;
    let mut writer = hound::WavWriter::create(
        output,
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut seed = 0x7f4a7c15_u32;
    let mut low = 0.0_f32;
    let length = seconds * rate;
    for i in 0..length {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let white = (seed as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32;
        low = low * 0.985 + white * 0.015;
        let value = match kind {
            "brown" => low * 3.0,
            "rain" => white * 0.5 + low,
            _ => white * 0.6,
        };
        let fade = (i.min(length - 1 - i) as f32 / (rate as f32 * 0.05)).min(1.0);
        writer.write_sample((value * fade * 0.2 * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_requests_preserve_paths_and_reject_remote_files() {
        assert_eq!(
            parse("/music play /tmp/a song.mp3")
                .unwrap()
                .source
                .as_deref(),
            Some("/tmp/a song.mp3")
        );
        assert!(parse("/music volume 101").is_err());
        assert!(parse("/music pause anything").is_err());
        assert!(parse("/music spotify play --option").is_err());
        assert!(parse("/music spotify play spotify:track:2fFlmlePz9hrVMv4LvdQxN").is_ok());
        assert!(parse("/music ambience ocean").is_err());
    }
    #[test]
    fn ambience_has_bounded_gain_and_fades_without_a_loop_click() {
        let dir = tempfile::tempdir().unwrap();
        for kind in ["brown", "rain", "white"] {
            let path = dir.path().join("test.wav");
            render_ambience(kind, &path, 1).unwrap();
            let mut wav = hound::WavReader::open(&path).unwrap();
            let samples: Vec<_> = wav.samples::<i16>().map(Result::unwrap).collect();
            assert_eq!(samples.len(), 44100);
            assert_eq!(samples[0], 0);
            assert_eq!(samples[44099], 0);
            assert!(samples.iter().any(|s| *s != 0));
            assert!(samples.iter().all(|s| s.unsigned_abs() < 12000));
        }
    }
}
