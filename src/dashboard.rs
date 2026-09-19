use crate::config::Settings;
use anyhow::{bail, Result};

pub const COMMANDS: &[(&str, &str)] = &[
    ("/settings", "Agents, models, wake, voice, chat, and speed"),
    ("/help", "Show commands"),
    ("/setup", "Guided setup in this screen"),
    ("/config", "Inspect settings; /config set KEY VALUE"),
    ("/tts", "Choose and test voices"),
    (
        "/stt",
        "Local STT test, /stt provider cartesia|local, or /stt engine parakeet",
    ),
    ("/jev", "Save a TypeSafe API key for Jev: /jev key"),
    ("/devices", "List microphones"),
    ("/connectors", "Show agent connections"),
    ("/analytics", "Lifetime and past-week cost/call breakdown"),
    ("/context", "Show approximate conversation tokens"),
    ("/compact", "Summarize Accessor-owned history now"),
    ("/update", "Check harness CLIs and apply updates"),
    ("/agent", "Open Codex; exit to return"),
    ("/events", "Show the event queue"),
    ("/organizer", "Show notes, alarms, and scheduled tasks"),
    ("/status", "Show current state"),
    ("/stop", "Cancel and sleep"),
    ("/sleep", "Close voice access"),
    ("/cancel", "Cancel current work"),
    ("/mute", "Mute microphone input"),
    ("/unmute", "Resume microphone input"),
    ("/approve", "Approve one request by number"),
    ("/deny", "Deny one request by number"),
    ("/quit", "Exit Accessor"),
];
pub fn help() -> String {
    COMMANDS
        .iter()
        .map(|(name, desc)| format!("{name:14} {desc}"))
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn matching(input: &str) -> Vec<(&'static str, &'static str)> {
    if !input.starts_with('/') || input.contains(char::is_whitespace) {
        return Vec::new();
    }
    COMMANDS
        .iter()
        .copied()
        .filter(|(name, _)| name.starts_with(input))
        .take(8)
        .collect()
}
pub fn summary(s: &Settings) -> String {
    format!("Wake code: {}\nWake mode: code required for every voice request\nSleep after: {} idle seconds (0 = never)\nSpoken replies: {}\nVoice provider: {}\nSpeaking speed: {:.2}×\nKokoro voice: {}\nChat view: {}\nPlugin harness: {} · model: {}\nCoding / everyday: {} / {}\nRouter: {}\nApprovals: {}\nLocal STT: {}\nConversation STT: {}\nCompaction: {} · {} @ ~{} tokens\nSettings: {}\nHome folder: {}\nCopy this setup: acc config locations (or /config locations)\nChange any setting: /config set KEY VALUE\nSee docs/PLATFORM.md",s.wake_code,s.idle_seconds,s.speak,s.tts.provider,s.tts.speed,s.tts.local_voice,s.chat,s.agent,s.model.as_deref().unwrap_or("default"),s.routing.coding,s.routing.routine,s.routing.router,s.approvals.reviewer,s.stt.engine,s.stt.conversation,s.routing.compaction_harness,s.routing.compaction_model,s.routing.compact_tokens,crate::config::path().map(|p|p.display().to_string()).unwrap_or_default(),crate::config::home().map(|p|p.display().to_string()).unwrap_or_default())
}
pub fn tts_help(s: &Settings) -> String {
    format!("Voice provider: {}\nSpeed: {:.2}× (0.6–1.5)\n/tts provider system|kokoro|cartesia|off\n/tts voice VOICE_ID        Set the selected provider's voice\n/tts speed 1.1            Speaking speed for local and Cartesia voices\n/tts test [sample text]    Hear a sample\n/tts voices               List neural voices\n/tts key                  Enter Cartesia key in a hidden field\nKokoro installation: python scripts/setup_tts.py",s.tts.provider,s.tts.speed)
}
pub enum LocalCommand {
    Settings,
    SettingsNav(String),
    Help,
    Setup,
    Config,
    Locations,
    Tts,
    Set(String, String),
    Speak(String),
    Secret(&'static str),
    Stt(bool),
    Utility(Vec<String>),
    Native,
    Analytics,
    Context,
    Compact,
    Update { check: bool },
}
pub fn parse(text: &str, s: &Settings) -> Result<Option<LocalCommand>> {
    let text = text.trim();
    let parts: Vec<_> = text.split_whitespace().collect();
    let value = match parts.as_slice() {
        ["/settings"] | ["/settings", "refresh"] => LocalCommand::Settings,
        ["/settings-nav", dir] => LocalCommand::SettingsNav((*dir).into()),
        ["/settings", "voices"] => return parse("/tts voices", s),
        ["/settings", "tts"] => return parse("/tts", s),
        ["/settings", key, value] => LocalCommand::Set((*key).into(), (*value).into()),
        ["/"] | ["/help"] => LocalCommand::Help,
        ["/setup"] => LocalCommand::Setup,
        ["/config"] | ["/config", "show"] => LocalCommand::Config,
        ["/config", "locations"] | ["/config", "path"] => LocalCommand::Locations,
        ["/config", "set", key, ..] => {
            let value = text
                .strip_prefix("/config")
                .unwrap()
                .trim_start()
                .strip_prefix("set")
                .unwrap()
                .trim_start();
            let value = value.strip_prefix(key).unwrap().trim();
            if value.is_empty() {
                bail!("Usage: /config set KEY VALUE");
            }
            LocalCommand::Set((*key).into(), value.trim_matches('"').into())
        }
        ["/tts"] | ["/tts", "setup"] => LocalCommand::Tts,
        ["/tts", "provider", provider] => {
            LocalCommand::Set("tts.provider".into(), (*provider).into())
        }
        ["/tts", "voice", voice] => LocalCommand::Set(
            if s.tts.provider == "kokoro" {
                "tts.local-voice"
            } else {
                "tts.voice"
            }
            .into(),
            (*voice).into(),
        ),
        ["/tts", "speed", speed] => LocalCommand::Set("tts.speed".into(), (*speed).into()),
        ["/tts", "test", ..] => LocalCommand::Speak(parts[2..].join(" ")),
        ["/tts", "key"] => LocalCommand::Secret("cartesia"),
        ["/tts", "voices"] => {
            if s.tts.provider != "cartesia" && s.tts.provider != "kokoro" {
                bail!("Choose /tts provider kokoro or cartesia first.");
            }
            LocalCommand::Utility(vec![
                "tts".into(),
                "voices".into(),
                "--provider".into(),
                s.tts.provider.clone(),
            ])
        }
        ["/devices"] => LocalCommand::Utility(vec!["devices".into()]),
        ["/connectors"] | ["/connectors", "status"] => {
            LocalCommand::Utility(vec!["connectors".into(), "status".into()])
        }
        ["/analytics"] => LocalCommand::Analytics,
        ["/context"] => LocalCommand::Context,
        ["/compact"] => LocalCommand::Compact,
        ["/update"] => LocalCommand::Update { check: false },
        ["/update", "check"] | ["/update", "--check"] => LocalCommand::Update { check: true },
        ["/connectors", "setup"] | ["/agent"] => LocalCommand::Native,
        ["/events"] => LocalCommand::Utility(vec!["events".into(), "status".into()]),
        ["/organizer"] | ["/alarms"] | ["/tasks"] | ["/notes"] => {
            LocalCommand::Utility(vec!["organizer".into(), "status".into()])
        }
        ["/stt", "provider", provider] => {
            LocalCommand::Set("stt.conversation".into(), (*provider).into())
        }
        ["/stt", "engine", engine] | ["/stt", "model", engine] => {
            LocalCommand::Set("stt.engine".into(), (*engine).into())
        }
        ["/stt"] | ["/stt", "test"] => LocalCommand::Stt(true),
        ["/stt", "off"] => LocalCommand::Stt(false),
        ["/jev", "key"] | ["/typesafe", "key"] => LocalCommand::Secret("typesafe"),
        ["/jev"] => LocalCommand::Secret("typesafe"),
        _ => return Ok(None),
    };
    Ok(Some(value))
}
pub struct Wizard {
    step: u8,
    draft: Settings,
}
impl Wizard {
    pub fn new(s: &Settings) -> Self {
        Self {
            step: 0,
            draft: s.clone(),
        }
    }
    pub fn question(&self) -> String {
        match self.step {
            0 => format!(
                "Setup 1/4 · Wake code [{}] — Enter keeps the current value; /cancel exits.",
                self.draft.wake_code
            ),
            1 => format!(
                "Setup 2/4 · Wake-listening seconds [{}], 0 for no timeout.",
                self.draft.idle_seconds
            ),
            2 => format!(
                "Setup 3/4 · Voice: system, kokoro, cartesia, off [{}].",
                self.draft.tts.provider
            ),
            _ => format!(
                "Setup 4/4 · Speak replies? yes/no [{}].",
                if self.draft.speak { "yes" } else { "no" }
            ),
        }
    }
    pub fn answer(&mut self, text: &str) -> Result<Option<Settings>> {
        let text = text.trim();
        let mut candidate = self.draft.clone();
        if !text.is_empty() {
            match self.step {
                0 => candidate.wake_code = text.into(),
                1 => candidate.idle_seconds = text.parse()?,
                2 => candidate.tts.provider = text.into(),
                _ => {
                    candidate.speak = match text.to_lowercase().as_str() {
                        "y" | "yes" | "true" => true,
                        "n" | "no" | "false" => false,
                        _ => bail!("Enter yes or no"),
                    }
                }
            }
        }
        candidate.validate()?;
        self.draft = candidate;
        self.step += 1;
        Ok(if self.step == 4 {
            Some(self.draft.clone())
        } else {
            None
        })
    }
}
pub fn utility(args: Vec<String>) -> tokio::task::JoinHandle<Result<String>> {
    tokio::spawn(async move {
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command.args(args).kill_on_drop(true);
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(120), command.output()).await??;
        let text = String::from_utf8_lossy(&output.stdout);
        let error = String::from_utf8_lossy(&output.stderr);
        Ok(format!("{text}{error}"))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_values_and_menu() {
        let Some(LocalCommand::Set(key, value)) = parse(
            "/config set microphone Microphone (Realtek)",
            &Settings::default(),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(key, "microphone");
        assert_eq!(value, "Microphone (Realtek)");
        assert_eq!(matching("/con").len(), 3);
        assert!(matching("/tts key secret").is_empty());
        let Some(LocalCommand::Set(key, value)) =
            parse("/stt provider cartesia", &Settings::default()).unwrap()
        else {
            panic!()
        };
        assert_eq!(key, "stt.conversation");
        assert_eq!(value, "cartesia");
        let Some(LocalCommand::Set(key, value)) =
            parse("/stt provider ink-2", &Settings::default()).unwrap()
        else {
            panic!()
        };
        assert_eq!(key, "stt.conversation");
        assert_eq!(value, "ink-2");
        let Some(LocalCommand::Set(key, value)) =
            parse("/stt engine parakeet", &Settings::default()).unwrap()
        else {
            panic!()
        };
        assert_eq!(key, "stt.engine");
        assert_eq!(value, "parakeet");
        assert!(matches!(
            parse("/analytics", &Settings::default()).unwrap(),
            Some(LocalCommand::Analytics)
        ));
        assert!(matches!(
            parse("/context", &Settings::default()).unwrap(),
            Some(LocalCommand::Context)
        ));
        assert!(matches!(
            parse("/jev key", &Settings::default()).unwrap(),
            Some(LocalCommand::Secret("typesafe"))
        ));
        assert!(matches!(
            parse("/update", &Settings::default()).unwrap(),
            Some(LocalCommand::Update { check: false })
        ));
        assert!(matches!(
            parse("/update check", &Settings::default()).unwrap(),
            Some(LocalCommand::Update { check: true })
        ));
    }
    #[test]
    fn wizard_validates_before_advancing() {
        let mut w = Wizard::new(&Settings::default());
        w.answer("42").unwrap();
        assert!(w.answer("9000").is_err());
        assert!(w.question().contains("2/4"));
        w.answer("0").unwrap();
        w.answer("kokoro").unwrap();
        let s = w.answer("yes").unwrap().unwrap();
        assert_eq!(s.wake_code, "42");
        assert_eq!(s.idle_seconds, 0);
        assert_eq!(s.tts.provider, "kokoro");
    }
}
