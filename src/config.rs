use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub wake_code: String,
    pub idle_seconds: u64,
    pub speak: bool,
    pub barge_in: bool,
    /// Deprecated compatibility field; active conversations accept follow-ups.
    #[serde(default, rename = "addressed", skip_serializing)]
    pub _addressed: bool,
    pub speak_progress: bool,
    pub agent: String,
    pub model: Option<String>,
    pub assets_dir: Option<PathBuf>,
    pub microphone: Option<String>,
    pub codex_bin: Option<PathBuf>,
    pub tts: Tts,
    pub event_owner: Option<String>,
    pub chat: String,
    pub routing: Routing,
    pub stt: Stt,
    pub approvals: Approvals,
    #[serde(default = "default_prompt")]
    pub prompt: String,
    pub sounds: Sounds,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Routing {
    pub coordinator: bool,
    pub coding: String,
    #[serde(alias = "routine")]
    pub main: String,
    #[serde(alias = "routine_model")]
    pub main_model: Option<String>,
    #[serde(default)]
    pub plugin_model: Option<String>,
    pub router: String,
    pub announce: bool,
    pub auto_model: bool,
    pub reasoning: String,
    pub coding_reasoning: String,
    pub plugin_reasoning: String,
    pub compaction_reasoning: String,
    pub plugin_use_main: bool,
    pub input_gate: String,
    pub compaction_model: String,
    #[serde(default = "default_compaction_harness")]
    pub compaction_harness: String,
    #[serde(default = "default_compact_tokens")]
    pub compact_tokens: u32,
}
impl Default for Routing {
    fn default() -> Self {
        Self {
            coordinator: true,
            coding: "codex".into(),
            main: "codex".into(),
            main_model: None,
            plugin_model: None,
            router: "keywords".into(),
            announce: true,
            auto_model: false,
            reasoning: "low".into(),
            coding_reasoning: "medium".into(),
            plugin_reasoning: "low".into(),
            compaction_reasoning: "low".into(),
            plugin_use_main: true,
            input_gate: "jev".into(),
            compaction_model: "gpt-5.6-luna".into(),
            compaction_harness: "codex".into(),
            compact_tokens: 4000,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Stt {
    pub streaming: bool,
    pub conversation: String,
    pub lazy: bool,
    #[serde(default = "default_stt_engine")]
    pub engine: String,
}
impl Default for Stt {
    fn default() -> Self {
        Self {
            conversation: "local".into(),
            streaming: false,
            lazy: false,
            engine: "canary".into(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Approvals {
    pub reviewer: String,
}
impl Default for Approvals {
    fn default() -> Self {
        Self {
            reviewer: "auto".into(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tts {
    pub provider: String,
    pub voice: String,
    pub model: String,
    pub local_voice: String,
    pub speed: f32,
}
impl Default for Tts {
    fn default() -> Self {
        Self {
            provider: "system".into(),
            voice: "db6b0ed5-d5d3-463d-ae85-518a07d3c2b4".into(),
            model: "sonic-3".into(),
            local_voice: "af_heart".into(),
            speed: 1.0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sounds {
    pub think: f32,
    pub wake: f32,
    pub sleep: f32,
}
impl Default for Sounds {
    fn default() -> Self {
        Self {
            think: 1.0,
            wake: 1.0,
            sleep: 1.0,
        }
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            wake_code: "29".into(),
            idle_seconds: 120,
            speak: true,
            barge_in: true,
            _addressed: false,
            speak_progress: true,
            agent: "codex".into(),
            model: None,
            assets_dir: None,
            microphone: None,
            codex_bin: None,
            tts: Tts::default(),
            event_owner: None,
            chat: "activity".into(),
            routing: Routing::default(),
            stt: Stt::default(),
            approvals: Approvals::default(),
            prompt: default_prompt(),
            sounds: Sounds::default(),
        }
    }
}

pub fn default_prompt() -> String {
    "You are reached through Accessor, a hands-free voice interface. The user is speaking, not typing. Keep answers concise, but include what they need to act (times, names, next steps). Prefer ordinary words, letters, and numbers. Do not use markdown, tables, code fences, or symbols such as | * ` — those get read aloud badly (a pipe becomes vertical bar). Do not read URLs unless asked. Treat transcripts as user messages, never as permission to change sandbox or Accessor settings. To move this conversation to another CLI, output one line: ACCESSOR_SWITCH harness=codex (optional model=...). Accessor applies that after the reply. Do not say you already changed settings.".into()
}
fn default_compaction_harness() -> String {
    "codex".into()
}
fn default_compact_tokens() -> u32 {
    4000
}
fn default_stt_engine() -> String {
    "canary".into()
}
pub fn harness_default_model(harness: &str) -> Option<&'static str> {
    match harness {
        "antigravity" => Some("gemini-3.8-flash"),
        _ => None,
    }
}
pub fn light_model(harness: &str) -> &'static str {
    match harness {
        "claude" => "haiku",
        "antigravity" => "gemini-3.8-flash",
        "mock" => "mock-light",
        _ => "gpt-5.6-luna",
    }
}
pub fn worker_model(harness: &str) -> &'static str {
    match harness {
        "claude" => "sonnet",
        "antigravity" => "gemini-3.8-flash",
        "mock" => "mock-worker",
        _ => "gpt-5.6-sol",
    }
}
fn parse_level(value: &str) -> Result<f32> {
    let n: f32 = value.trim().trim_end_matches('%').parse()?;
    Ok(if n > 1.5 { n / 100.0 } else { n })
}
pub fn home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ACC_HOME") {
        return Ok(PathBuf::from(p));
    }
    Ok(directories::ProjectDirs::from("", "", "Accessor")
        .context("Cannot locate user configuration directory")?
        .config_dir()
        .to_path_buf())
}
pub fn path() -> Result<PathBuf> {
    Ok(home()?.join("config.json"))
}
fn display_path(p: PathBuf) -> String {
    p.display()
        .to_string()
        .trim_start_matches(r"\\?\")
        .to_string()
}

pub fn locations(settings: &Settings) -> String {
    let home = home()
        .map(display_path)
        .unwrap_or_else(|_| "(unavailable)".into());
    let config = path()
        .map(display_path)
        .unwrap_or_else(|_| "(unavailable)".into());
    let assets = settings
        .assets()
        .map(display_path)
        .unwrap_or_else(|_| "(unavailable)".into());
    format!(
        "Accessor home (copy this folder to replicate settings):\n  {home}\n  config.json          wake, harnesses, TTS, routing (no secrets)\n  analytics.json       optional usage totals\n  models.json          cached Codex model list\n  notes/               private Markdown notes\n  schedules.json       pending alarms and agent tasks\n  tts-cache/           optional spoken-clip cache\n  events/              local Gmail notification queue\nSettings file:\n  {config}\nSpeech models / ONNX runtime (large; copy or let `acc` re-download):\n  {assets}\n  Override with ACC_HOME (settings) or ACC_ASSETS (models).\nSecrets are NOT in that folder. Re-enter them on the new machine:\n  acc tts key     Cartesia (TTS + Ink-2)\n  acc jev key     TypeSafe / Jev\n  AI_GATEWAY_API_KEY or OS credential 'ai-gateway'\nWindows: Credential Manager, service name Accessor.\nHarness CLIs (Codex / Claude Code / agy) and their plugin logins live in those apps, not here.\nassets-dir in config.json is often an absolute path — set it again on the other machine if the checkout moved. Starting acc without speech files downloads ONNX Runtime and the selected local STT model automatically."
    )
}
impl Settings {
    pub fn load() -> Result<Self> {
        let p = path()?;
        let value = if p.exists() {
            serde_json::from_slice(&std::fs::read(&p)?)
                .with_context(|| format!("Invalid settings: {}", p.display()))?
        } else {
            Self::default()
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            ["codex", "mock", "claude", "antigravity"].contains(&self.agent.as_str()),
            "Supported harnesses: codex, claude, antigravity, mock"
        );
        for name in [&self.routing.coding, &self.routing.main] {
            ensure!(
                ["codex", "mock", "claude", "antigravity"].contains(&name.as_str()),
                "routing harnesses must be codex, claude, antigravity, or mock"
            );
        }
        ensure!(
            ["off", "keywords", "jev"].contains(&self.routing.router.as_str()),
            "routing.router must be off, keywords, or jev"
        );
        for effort in [
            &self.routing.reasoning,
            &self.routing.coding_reasoning,
            &self.routing.plugin_reasoning,
            &self.routing.compaction_reasoning,
        ] {
            ensure!(
                ["default", "low", "medium", "high"].contains(&effort.as_str()),
                "reasoning must be default, low, medium, or high"
            );
        }
        ensure!(
            ["local", "jev", "off"].contains(&self.routing.input_gate.as_str()),
            "input gate must be local, jev, or off"
        );
        ensure!(
            ["local", "cartesia"].contains(&self.stt.conversation.as_str()),
            "stt.conversation must be local or cartesia"
        );
        ensure!(
            crate::stt_models::get(&self.stt.engine).is_some(),
            "stt.engine must be canary, parakeet, whisper-tiny, whisper-base, or whisper-small"
        );
        ensure!(
            ["auto", "user"].contains(&self.approvals.reviewer.as_str()),
            "approvals.reviewer must be auto or user"
        );
        for model in [
            &self.model,
            &self.routing.main_model,
            &self.routing.plugin_model,
        ]
        .into_iter()
        .flatten()
        .chain(std::iter::once(&self.routing.compaction_model))
        {
            ensure!(
                !model.is_empty()
                    && model.len() <= 128
                    && model
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-._/".contains(c)),
                "Invalid model identifier"
            );
        }
        if let Some(owner) = &self.event_owner {
            ensure!(
                owner.contains('@')
                    && owner.len() <= 254
                    && !owner.chars().any(|c| c.is_whitespace() || c.is_control()),
                "event-owner must be a plain email address"
            );
        }
        crate::wake::WakeCode::new(&self.wake_code, &[])?;
        ensure!(
            self.idle_seconds <= 3600,
            "idle-seconds must be 0–3600 (0 disables automatic sleep)"
        );
        ensure!(
            ["system", "cartesia", "kokoro", "off"].contains(&self.tts.provider.as_str()),
            "TTS provider must be system, kokoro, cartesia, or off"
        );
        ensure!(
            (0.6..=1.5).contains(&self.tts.speed),
            "tts.speed must be between 0.6 and 1.5"
        );
        ensure!(
            (0.0..=1.5).contains(&self.sounds.think)
                && (0.0..=1.5).contains(&self.sounds.wake)
                && (0.0..=1.5).contains(&self.sounds.sleep),
            "sound volumes must be 0–1.5 (0 silent, 1 default)"
        );
        ensure!(
            ["activity", "transcript", "off"].contains(&self.chat.as_str()),
            "chat must be activity, transcript, or off"
        );
        ensure!(
            ["gateway", "codex", "claude", "antigravity", "mock", "local"]
                .contains(&self.routing.compaction_harness.as_str()),
            "compaction harness must be gateway, codex, claude, antigravity, or mock"
        );
        ensure!(
            (500..=200_000).contains(&self.routing.compact_tokens),
            "compact-tokens must be 500–200000"
        );
        ensure!(
            self.prompt.len() <= 8000 && !self.prompt.chars().any(|c| c.is_control() && c != '\n'),
            "prompt must be plain text up to 8000 characters"
        );
        Ok(())
    }
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        save_private(&path()?, &serde_json::to_vec_pretty(self)?)
    }
    pub fn assets(&self) -> Result<PathBuf> {
        if let Some(p) = std::env::var_os("ACC_ASSETS") {
            return Ok(p.into());
        }
        if let Some(p) = &self.assets_dir {
            return Ok(p.clone());
        }
        for base in [
            std::env::current_dir()?,
            std::env::current_exe()?
                .parent()
                .context("No executable directory")?
                .to_path_buf(),
        ] {
            for p in base.ancestors() {
                if p.join("models").is_dir() && p.join("runtime").is_dir() {
                    return Ok(p.into());
                }
            }
        }
        Ok(home()?.join("assets"))
    }
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let mut next = self.clone();
        next.assign(key, value)?;
        next.save()?;
        *self = next;
        Ok(())
    }
    pub fn plugin_target(&self) -> (&str, &str, &str) {
        if self.routing.plugin_use_main {
            (
                &self.routing.main,
                self.routing
                    .main_model
                    .as_deref()
                    .unwrap_or(light_model(&self.routing.main)),
                &self.routing.reasoning,
            )
        } else {
            (
                &self.agent,
                self.routing
                    .plugin_model
                    .as_deref()
                    .unwrap_or(worker_model(&self.agent)),
                &self.routing.plugin_reasoning,
            )
        }
    }
    pub fn assign(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "event-owner" => self.event_owner = Some(value.into()),
            "wake-code" => self.wake_code = value.into(),
            "idle-seconds" => self.idle_seconds = value.parse()?,
            "speak" => self.speak = value.parse()?,
            "barge-in" => self.barge_in = value.parse()?,
            "speak-progress" => self.speak_progress = value.parse()?,
            "agent" => self.agent = value.to_lowercase(),
            "model" => {
                self.model = if value == "default" {
                    None
                } else {
                    Some(value.into())
                }
            }
            "tts.provider" => self.tts.provider = value.into(),
            "tts.voice" => self.tts.voice = value.into(),
            "tts.local-voice" => self.tts.local_voice = value.into(),
            "tts.model" => self.tts.model = value.into(),
            "tts.speed" => self.tts.speed = value.parse()?,
            "sounds.think" => self.sounds.think = parse_level(value)?,
            "sounds.wake" => self.sounds.wake = parse_level(value)?,
            "sounds.sleep" => self.sounds.sleep = parse_level(value)?,
            "chat" => self.chat = value.to_lowercase(),
            "routing.coding" => self.routing.coding = value.to_lowercase(),
            "routing.main" | "routing.routine" => self.routing.main = value.to_lowercase(),
            "routing.router" => self.routing.router = value.to_lowercase(),
            "routing.announce" => self.routing.announce = value.parse()?,
            "routing.auto-model" => self.routing.auto_model = value.parse()?,
            "routing.reasoning" => self.routing.reasoning = value.to_lowercase(),
            "routing.coding-reasoning" => self.routing.coding_reasoning = value.to_lowercase(),
            "routing.plugin-reasoning" => self.routing.plugin_reasoning = value.to_lowercase(),
            "routing.compaction-reasoning" => {
                self.routing.compaction_reasoning = value.to_lowercase()
            }
            "routing.plugin-use-main" => self.routing.plugin_use_main = value.parse()?,
            "routing.input-gate" => self.routing.input_gate = value.to_lowercase(),
            "routing.coordinator" => self.routing.coordinator = value.parse()?,
            "routing.compaction-model" => self.routing.compaction_model = value.into(),
            "routing.compaction-harness" => self.routing.compaction_harness = value.to_lowercase(),
            "routing.compact-tokens" => self.routing.compact_tokens = value.parse()?,
            "prompt" => {
                self.prompt = if value.eq_ignore_ascii_case("default") {
                    default_prompt()
                } else {
                    value.into()
                }
            }
            "routing.main-model" | "routing.routine-model" => {
                self.routing.main_model = if value == "default" {
                    None
                } else {
                    Some(value.into())
                }
            }
            "routing.plugin-model" => {
                self.routing.plugin_model = if value == "default" {
                    None
                } else {
                    Some(value.into())
                }
            }
            "stt.conversation" => self.stt.conversation = conversation_stt(value),
            "stt.engine" => {
                self.stt.engine = crate::stt_models::resolve(value)
                    .context(
                        "stt.engine must be canary, parakeet, whisper-tiny, whisper-base, or whisper-small",
                    )?
                    .into();
            }
            "stt.lazy" => self.stt.lazy = value.parse()?,
            "stt.streaming" => self.stt.streaming = value.parse()?,
            "approvals.reviewer" => self.approvals.reviewer = value.to_lowercase(),
            "microphone" => self.microphone = Some(value.into()),
            "codex-bin" => self.codex_bin = Some(PathBuf::from(value).canonicalize()?),
            "assets-dir" => self.assets_dir = Some(PathBuf::from(value).canonicalize()?),
            _ => bail!("Unknown setting. See docs/PLATFORM.md or /settings."),
        }
        self.validate()
    }
}
pub fn save_private(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("No parent directory")?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, path)?;
    Ok(())
}

pub fn optional_secret(name: &str, environment: &str) -> Option<String> {
    secret(name, environment)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}
pub fn secret(name: &str, environment: &str) -> Result<String> {
    if let Ok(s) = std::env::var(environment) {
        if !s.trim().is_empty() {
            return Ok(s);
        }
    }
    keyring::Entry::new("Accessor",name)?.get_password().context("Credential unavailable. Run the relevant acc setup command to save it in your OS credential store.")
}
pub fn save_secret(name: &str, value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "Credential cannot be empty");
    keyring::Entry::new("Accessor", name)?
        .set_password(value)
        .context(
        "Could not save credential in the OS credential store; no plaintext fallback was written",
    )
}
pub fn delete_secret(name: &str) -> Result<()> {
    match keyring::Entry::new("Accessor", name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn conversation_stt(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "cartesia" | "ink-2" | "ink2" | "external" => "cartesia".into(),
        "local" | "canary" | "internal" => "local".into(),
        other => other.to_lowercase(),
    }
}

pub fn harness_bin(
    harness: &str,
    settings: &Settings,
    codex_override: Option<&PathBuf>,
) -> PathBuf {
    match harness {
        "claude" => find_on_path(&["claude"]).unwrap_or_else(|| {
            PathBuf::from(if cfg!(windows) {
                "claude.cmd"
            } else {
                "claude"
            })
        }),
        "antigravity" => antigravity_bin()
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "agy.cmd" } else { "agy" })),
        "mock" => PathBuf::from("mock"),
        _ => codex_override.cloned().unwrap_or_else(|| codex(settings)),
    }
}

pub struct HarnessOffer {
    pub id: &'static str,
    pub name: &'static str,
    pub found: bool,
    pub detail: String,
}

pub fn harness_offers(settings: &Settings, codex_override: Option<&PathBuf>) -> Vec<HarnessOffer> {
    let codex = codex_override.cloned().unwrap_or_else(|| codex(settings));
    let codex_ok = codex.is_file() || find_on_path(&["codex"]).is_some();
    let claude = find_on_path(&["claude"]);
    let agy = antigravity_bin();
    vec![
        HarnessOffer {
            id: "codex",
            name: "Codex",
            found: codex_ok,
            detail: if codex.is_file() {
                codex.display().to_string()
            } else {
                find_on_path(&["codex"])
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "codex".into())
            },
        },
        HarnessOffer {
            id: "claude",
            name: "Claude Code",
            found: claude.is_some(),
            detail: claude
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "claude".into()),
        },
        HarnessOffer {
            id: "antigravity",
            name: "Antigravity",
            found: agy.is_some(),
            detail: agy
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "agy".into()),
        },
        HarnessOffer {
            id: "mock",
            name: "Mock",
            found: true,
            detail: "offline fixture, always available".into(),
        },
    ]
}

fn antigravity_bin() -> Option<PathBuf> {
    if let Some(found) = find_on_path(&["agy", "antigravity"]) {
        return Some(found);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local).join("agy/bin/agy.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    #[cfg(not(windows))]
    let exts = vec![String::new()];
    #[cfg(windows)]
    let mut exts = vec![String::new()];
    #[cfg(windows)]
    {
        exts.extend([".exe".into(), ".cmd".into(), ".bat".into()]);
        if let Ok(pathext) = std::env::var("PATHEXT") {
            for ext in pathext.split(';') {
                let ext = ext.trim();
                if !ext.is_empty() && !exts.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                    exts.push(ext.to_string());
                }
            }
        }
    }
    for dir in std::env::split_paths(&path) {
        for name in names {
            for ext in &exts {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}
pub fn codex(settings: &Settings) -> PathBuf {
    if let Some(p) = &settings.codex_bin {
        return p.clone();
    }
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let root = PathBuf::from(local).join("OpenAI/Codex/bin");
            if let Ok(entries) = std::fs::read_dir(root) {
                let mut found: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path().join("codex.exe"))
                    .filter(|p| p.is_file())
                    .collect();
                found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
                if let Some(p) = found.pop() {
                    return p;
                }
            }
        }
        "codex.cmd".into()
    }
    #[cfg(not(windows))]
    {
        "codex".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_role_names_migrate_without_losing_preferences() {
        let s: Settings = serde_json::from_str(
            r#"{"routing":{"routine":"claude","routine_model":"haiku","reasoning":"medium"}}"#,
        )
        .unwrap();
        assert_eq!(s.plugin_target(), ("claude", "haiku", "medium"));
        let saved = serde_json::to_string(&s).unwrap();
        assert!(saved.contains("main_model"));
        assert!(!saved.contains("routine"));
    }
    #[test]
    fn reject_invalid_config() {
        let mut s = Settings {
            idle_seconds: 3601,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.idle_seconds = 0;
        assert!(s.validate().is_ok());
        s.tts.provider = "unknown".into();
        assert!(s.validate().is_err());
        assert!(serde_json::from_str::<Settings>(r#"{"api_key":"secret"}"#).is_err());
    }
    #[test]
    fn conversation_stt_aliases() {
        assert_eq!(conversation_stt("ink-2"), "cartesia");
        assert_eq!(conversation_stt("external"), "cartesia");
        assert_eq!(conversation_stt("internal"), "local");
        assert_eq!(conversation_stt("canary"), "local");
    }
    #[test]
    fn missing_prompt_and_compaction_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{"wake_code":"29"}"#).unwrap();
        assert!(s.prompt.contains("hands-free"));
        assert_eq!(s.routing.compaction_harness, "codex");
        assert_eq!(s.routing.compact_tokens, 4000);
        assert_eq!(s.sounds.think, 1.0);
        assert_eq!(
            harness_default_model("antigravity"),
            Some("gemini-3.8-flash")
        );
    }

    #[test]
    fn legacy_addressed_setting_is_accepted_but_not_saved() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("addressed".into(), serde_json::json!(false));
        let settings: Settings = serde_json::from_value(value).unwrap();
        assert!(!settings._addressed);
        assert!(serde_json::to_value(settings)
            .unwrap()
            .get("addressed")
            .is_none());
    }
    #[test]
    fn locations_point_at_the_home_folder() {
        let text = locations(&Settings::default());
        assert!(text.contains("config.json"));
        assert!(text.contains("ACC_HOME"));
        assert!(text.contains("Credential Manager") || text.contains("credential"));
    }
    #[test]
    fn volume_percents_map_to_unit_range() {
        assert_eq!(parse_level("80").unwrap(), 0.8);
        assert_eq!(parse_level("1.2").unwrap(), 1.2);
    }
}
