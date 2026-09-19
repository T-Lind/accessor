use crate::{config::Settings, connectors::Model};
use anyhow::{bail, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Voice,
    Speech,
    Harnesses,
    Display,
    Tests,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Home => "Settings",
            Self::Voice => "Voice",
            Self::Speech => "Speech",
            Self::Harnesses => "Harnesses",
            Self::Display => "Display",
            Self::Tests => "Tests",
        }
    }
}

pub struct Panel {
    page: Page,
    cursor: usize,
    field: Option<&'static str>,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            page: Page::Home,
            cursor: 0,
            field: None,
        }
    }
}
pub enum Answer {
    Show,
    Command(String),
    Close,
}

const ROUTERS: &[&str] = &["keywords", "jev", "off"];
const REASONING: &[&str] = &["default", "low", "medium", "high"];
const REVIEWERS: &[&str] = &["auto", "user"];
const STT_MODES: &[&str] = &["local", "cartesia"];
const CHATS: &[&str] = &["activity", "transcript", "off"];
struct Row {
    label: String,
    action: Action,
}
enum Action {
    Open(Page),
    Toggle(&'static str),
    Cycle(&'static str, &'static [&'static str]),
    Edit(&'static str),
    Run(&'static str),
    Close,
}

fn rows(page: Page, s: &Settings, connected: bool) -> Vec<Row> {
    match page {
        Page::Home => vec![
            row("Voice", "wake, barge-in, idle", Action::Open(Page::Voice)),
            row("Speech", "TTS, STT, speed", Action::Open(Page::Speech)),
            row(
                "Harnesses",
                "plugin, coding, everyday",
                Action::Open(Page::Harnesses),
            ),
            row(
                "Display",
                "chat view, identity code",
                Action::Open(Page::Display),
            ),
            row("Tests", "mic, voice, connectors", Action::Open(Page::Tests)),
            row("Close", "Esc", Action::Close),
        ],
        Page::Voice => vec![
            row("Wake code", &s.wake_code, Action::Edit("wake-code")),
            row(
                "Idle timeout",
                &format!("{} s", s.idle_seconds),
                Action::Edit("idle-seconds"),
            ),
            row("Barge-in", on(s.barge_in), Action::Toggle("barge-in")),
            row(
                "Think warble",
                &format!("{:.0}%", s.sounds.think * 100.0),
                Action::Edit("sounds.think"),
            ),
            row(
                "Wake chime",
                &format!("{:.0}%", s.sounds.wake * 100.0),
                Action::Edit("sounds.wake"),
            ),
            row(
                "Sleep chime",
                &format!("{:.0}%", s.sounds.sleep * 100.0),
                Action::Edit("sounds.sleep"),
            ),
            row("Back", "categories", Action::Open(Page::Home)),
        ],
        Page::Speech => vec![
            row("Speak replies", on(s.speak), Action::Toggle("speak")),
            row(
                "Speak progress",
                on(s.speak_progress),
                Action::Toggle("speak-progress"),
            ),
            row(
                "TTS provider",
                &s.tts.provider,
                Action::Edit("tts.provider"),
            ),
            row(
                "Voice ID",
                voice_id(s),
                Action::Edit(if s.tts.provider == "kokoro" {
                    "tts.local-voice"
                } else {
                    "tts.voice"
                }),
            ),
            row(
                "Speed",
                &format!("{:.2}×", s.tts.speed),
                Action::Edit("tts.speed"),
            ),
            row(
                "Local STT model",
                &engine_label(s),
                Action::Edit("stt.engine"),
            ),
            row(
                "After wake STT",
                if s.stt.conversation == "cartesia" {
                    "external · Cartesia Ink-2"
                } else {
                    "internal · same local model"
                },
                Action::Cycle("stt.conversation", STT_MODES),
            ),
            row(
                "Compute saving",
                if s.stt.lazy {
                    "load local STT on speech, free when asleep"
                } else {
                    "keep local STT in RAM"
                },
                Action::Toggle("stt.lazy"),
            ),
            row("Test voice", "play a sample", Action::Run("/tts test")),
            row("List voices", "", Action::Run("/tts voices")),
            row(
                "Cartesia API key",
                "hidden credential",
                Action::Run("/tts key"),
            ),
        ],
        Page::Harnesses => vec![
            row(
                "Plugin harness",
                &format!(
                    "{} · {}",
                    harness_label(s, &s.agent),
                    if connected {
                        "connected"
                    } else {
                        "connects on use"
                    }
                ),
                Action::Edit("agent"),
            ),
            row(
                "Plugin model",
                s.routing
                    .plugin_model
                    .as_deref()
                    .unwrap_or("harness default"),
                Action::Edit("routing.plugin-model"),
            ),
            row(
                "Coding harness",
                &harness_label(s, &s.routing.coding),
                Action::Edit("routing.coding"),
            ),
            row(
                "Coding model",
                s.model.as_deref().unwrap_or("harness default"),
                Action::Edit("model"),
            ),
            row(
                "Everyday harness",
                &harness_label(s, &s.routing.routine),
                Action::Edit("routing.routine"),
            ),
            row(
                "Everyday model",
                s.routing
                    .routine_model
                    .as_deref()
                    .unwrap_or("harness default"),
                Action::Edit("routing.routine-model"),
            ),
            row(
                "Router",
                &s.routing.router,
                Action::Cycle("routing.router", ROUTERS),
            ),
            row(
                "Jev auto-select",
                &jev_status(s),
                Action::Toggle("routing.auto-model"),
            ),
            row(
                "TypeSafe Jev key",
                "hidden credential",
                Action::Run("/jev key"),
            ),
            row(
                "Reasoning",
                &s.routing.reasoning,
                Action::Cycle("routing.reasoning", REASONING),
            ),
            row(
                "Approvals",
                if s.approvals.reviewer == "auto" {
                    "auto-review classifier"
                } else {
                    "ask me every time"
                },
                Action::Cycle("approvals.reviewer", REVIEWERS),
            ),
            row(
                "Compaction provider",
                &s.routing.compaction_harness,
                Action::Edit("routing.compaction-harness"),
            ),
            row(
                "Compaction model",
                &s.routing.compaction_model,
                Action::Edit("routing.compaction-model"),
            ),
            row(
                "Compact around",
                &format!("~{} tokens", s.routing.compact_tokens),
                Action::Edit("routing.compact-tokens"),
            ),
            row("Connected apps", "", Action::Run("/connectors")),
            row(
                "Harness login / plugins",
                "",
                Action::Run("/connectors setup"),
            ),
        ],
        Page::Display => vec![
            row("Chat view", chat_mode(s), Action::Cycle("chat", CHATS)),
            row(
                "Say harness code first",
                on(s.routing.announce),
                Action::Toggle("routing.announce"),
            ),
            row(
                "Voice metaprompt",
                &prompt_preview(&s.prompt),
                Action::Edit("prompt"),
            ),
        ],
        Page::Tests => vec![
            row(
                "Transcription test",
                "show every utterance",
                Action::Run("/stt test"),
            ),
            row("Test voice", "", Action::Run("/tts test")),
            row(
                "Analytics",
                "lifetime + past week",
                Action::Run("/analytics"),
            ),
            row(
                "Harness updates",
                "check Codex / Claude / agy",
                Action::Run("/update"),
            ),
        ],
    }
}
fn hint(action: &Action) -> &'static str {
    match action {
        Action::Open(Page::Voice) => {
            "Wake code (required for every voice request), idle sleep, and barge-in."
        }
        Action::Open(Page::Speech) => {
            "TTS voice, speed, and which transcriber runs after you are already awake."
        }
        Action::Open(Page::Harnesses) => {
            "Plugin CLI (email, calendar, docs), coding vs everyday, routing, and compaction."
        }
        Action::Open(Page::Display) => {
            "What the chat shows, spoken identity letters, and the editable voice metaprompt."
        }
        Action::Open(Page::Tests) => {
            "Live mic transcription test, a sample of the current voice, and usage analytics."
        }
        Action::Open(Page::Home) => "Return to the category list.",
        Action::Toggle("barge-in") => {
            "When on, new speech can interrupt the agent. When off, wait or /cancel."
        }
        Action::Toggle("speak") => "Speak agent replies out loud. Off keeps them on screen only.",
        Action::Toggle("speak-progress") => {
            "Read tool/progress lines while the agent works, not just the final answer."
        }
        Action::Toggle("stt.lazy") => {
            "Keep the mic and VAD running, but load the selected local STT only when speech starts and free it after sleep."
        }
        Action::Toggle("routing.announce") => {
            "Spoken letters (A. D.) at most once every 5 minutes. Replies in between skip the identity prefix."
        }
        Action::Toggle("routing.auto-model") => {
            "If a TypeSafe key exists, Jev uses recent conversation and the previous route to select plugin, coding, or everyday—and therefore that role's model."
        }
        Action::Cycle("stt.conversation", _) => {
            "Wake is always on-device with the selected local model. After GREEN, Ink-2 only runs on a finished VAD clip if that model heard a real word — never a live stream."
        }
        Action::Cycle("chat", _) => {
            "Activity shows tools plus replies. Transcript shows everything. Off hides commentary."
        }
        Action::Cycle("routing.router", _) => {
            "keywords: plugins (email/calendar/docs) then code-like turns then everyday. jev: TypeSafe classifies those three. off: always the everyday harness."
        }
        Action::Cycle("routing.reasoning", _) => {
            "Codex turn effort, and Antigravity --effort (required with --model; default maps to low). Say “use high reasoning” to change it; agent replies cannot."
        }
        Action::Cycle("approvals.reviewer", _) => {
            "auto: Codex Guardian/auto-review handles sandbox asks. user: you type /approve N."
        }
        Action::Edit("wake-code") => "Digits you say to wake Accessor, e.g. 29 or hey 29.",
        Action::Edit("idle-seconds") => {
            "Seconds to wait for speech after a bare wake code. 0 waits indefinitely."
        }
        Action::Edit("agent") => {
            "Plugin CLI: email, calendar, Google Docs, and other connected apps. Used when the turn needs those connectors."
        }
        Action::Edit("routing.coding") => "CLI for repos, diffs, tests, and refactors.",
        Action::Edit("routing.routine") => {
            "Everyday CLI for general chat: time, planning, questions that are not code and not a plugin."
        }
        Action::Edit("model") => {
            "Models for the coding CLI. Antigravity lists Gemini; Codex uses its account catalog."
        }
        Action::Edit("routing.plugin-model") => {
            "Models for the plugin CLI — the list matches that harness, not the coding one."
        }
        Action::Edit("routing.routine-model") => {
            "Models for the everyday CLI. If everyday is Antigravity, this is Gemini/Claude, not Codex."
        }
        Action::Edit("routing.compaction-harness") => {
            "Pick Gateway or an installed harness, then a model from that provider. Uninstalled CLIs are omitted."
        }
        Action::Edit("routing.compaction-model") => {
            "Model used to summarize Accessor-owned history. Listed from the compaction provider you selected first."
        }
        Action::Edit("routing.compact-tokens") => {
            "Approximate token budget for Accessor-owned history. Over this, /compact and auto-compact run."
        }
        Action::Edit("prompt") => {
            "Saved instructions sent to Codex, Claude, and Antigravity. Type default to restore the hands-free voice prompt."
        }
        Action::Edit("tts.provider") => "How replies are spoken: system, kokoro, cartesia, or off.",
        Action::Edit("tts.voice") | Action::Edit("tts.local-voice") => {
            "Voice id for the current TTS provider."
        }
        Action::Edit("tts.speed") => "Speaking speed 0.6–1.5 for Kokoro and Cartesia.",
        Action::Edit("stt.engine") => {
            "Opens the full local STT list. Esc backs out with no download. Only the model you pick is confirmed, and only if it is not already installed."
        },
        Action::Edit("sounds.think") => {
            "Volume of the looping warble while the agent is working. 0 silent, 1 default, 1.5 loud. Percent values like 80 also work."
        }
        Action::Edit("sounds.wake") => "Volume of the wake chime. 0 silent, 1 default.",
        Action::Edit("sounds.sleep") => "Volume of the sleep chime. 0 silent, 1 default.",
        Action::Run("/tts test") => "Play a short sample with the current TTS settings.",
        Action::Run("/tts voices") => "List neural voices for Kokoro or Cartesia.",
        Action::Run("/tts key") => "Save a Cartesia API key in the OS credential store (TTS and Ink-2).",
        Action::Run("/jev key") => {
            "Save a TypeSafe API key in the OS credential store (not settings.json). Needed for Jev routing. TYPESAFE_API_KEY also works."
        },
        Action::Run("/stt test") => {
            "Show every transcript with no agent. Wake still uses the selected local STT model."
        }
        Action::Run("/connectors") => "Show the selected harness's connected apps.",
        Action::Run("/connectors setup") => "Open the native CLI to log in or add plugins.",
        Action::Run("/analytics") => {
            "Show lifetime and past-week calls, including Jev, with estimated cost bars."
        }
        Action::Run("/update") => {
            "Run each found harness's own updater (codex update, claude update, agy update). /update check only prints versions."
        }
        Action::Close => "Leave settings and unmute the microphone.",
        _ => "Enter to change this setting. Esc goes back.",
    }
}

fn row(label: &str, value: &str, action: Action) -> Row {
    Row {
        label: if value.is_empty() {
            label.into()
        } else {
            format!("{label:<28} {value}")
        },
        action,
    }
}
fn prompt_preview(prompt: &str) -> String {
    let one = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= 42 {
        one
    } else {
        format!("{}…", one.chars().take(41).collect::<String>())
    }
}
fn model_picker(key: &str, s: &Settings, discovered: &[Model]) -> String {
    let (role, harness) = match key {
        "routing.plugin-model" => ("Plugin", s.agent.as_str()),
        "routing.routine-model" => ("Everyday", s.routing.routine.as_str()),
        _ => ("Coding", s.routing.coding.as_str()),
    };
    let catalog = harness_models(harness, discovered);
    let list = if catalog.is_empty() {
        "(no catalog for this CLI — type an id, or 0 for default)".to_string()
    } else {
        catalog
            .iter()
            .enumerate()
            .map(|(i, m)| format!("{}  {} ({})", i + 1, m.name, m.id))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("{role} model for {harness} · 0 = harness default\n{list}\nType a number or id. Esc returns.")
}
fn catalog_for_key(key: &str, s: &Settings, discovered: &[Model]) -> Vec<Model> {
    let harness = match key {
        "routing.plugin-model" => s.agent.as_str(),
        "routing.routine-model" => s.routing.routine.as_str(),
        _ => s.routing.coding.as_str(),
    };
    harness_models(harness, discovered)
}
fn harness_models(harness: &str, discovered: &[Model]) -> Vec<Model> {
    match harness {
        "codex" => {
            let mut list = discovered.to_vec();
            for id in ["gpt-5.6-luna", "gpt-5.4-sol"] {
                if !list.iter().any(|m| m.id == id) {
                    list.push(Model {
                        id: id.into(),
                        name: id.into(),
                    });
                }
            }
            list
        }
        "claude" => [
            "claude-haiku-4-5",
            "claude-haiku-3-5",
            "claude-sonnet-4-6",
            "claude-opus-4-6",
        ]
        .into_iter()
        .map(|id| Model {
            id: id.into(),
            name: id.into(),
        })
        .collect(),
        "antigravity" => [
            "gemini-3.8-flash",
            "gemini-3.5-flash",
            "gemini-3.5-pro",
            "claude-sonnet-4-6",
        ]
        .into_iter()
        .map(|id| Model {
            id: id.into(),
            name: id.into(),
        })
        .collect(),
        "mock" => Vec::new(),
        _ => [
            "google/gemini-3.5-flash",
            "google/gemini-3-flash",
            "openai/gpt-5.6-luna",
            "openai/gpt-5.4",
            "anthropic/claude-haiku-4.5",
            "anthropic/claude-sonnet-4.6",
        ]
        .into_iter()
        .map(|id| Model {
            id: id.into(),
            name: id.into(),
        })
        .collect(),
    }
}
fn compaction_models(harness: &str, models: &[Model]) -> Vec<Model> {
    harness_models(harness, models)
}
fn compaction_providers(s: &Settings) -> Vec<(&'static str, &'static str)> {
    let mut list = Vec::new();
    if crate::config::optional_secret("ai-gateway", "AI_GATEWAY_API_KEY").is_some() {
        list.push(("gateway", "AI Gateway"));
    }
    for h in crate::config::harness_offers(s, None) {
        if h.found && h.id != "mock" {
            list.push((h.id, h.name));
        }
    }
    if list.is_empty() {
        list.push(("local", "Local extractive trim"));
    }
    list
}
fn jev_status(s: &Settings) -> String {
    if !s.routing.auto_model {
        return "off".into();
    }
    if crate::route::jev_available() {
        "on · TypeSafe key saved; classifies each turn".into()
    } else {
        "on · no TypeSafe key, so keywords still run".into()
    }
}
fn harness_label(s: &Settings, id: &str) -> String {
    crate::config::harness_offers(s, None)
        .into_iter()
        .find(|h| h.id == id)
        .map(|h| {
            if h.found {
                format!("{} · found", h.name)
            } else {
                format!("{} · not on PATH", h.name)
            }
        })
        .unwrap_or_else(|| id.to_string())
}
fn on(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}
fn voice_id(s: &Settings) -> &str {
    match s.tts.provider.as_str() {
        "kokoro" => s.tts.local_voice.as_str(),
        "cartesia" => s.tts.voice.as_str(),
        _ => "OS default",
    }
}
fn chat_mode(s: &Settings) -> &'static str {
    match s.chat.as_str() {
        "transcript" => "full transcript",
        "off" => "tools only",
        _ => "activity (tools + replies)",
    }
}
fn current_value<'a>(key: &str, s: &'a Settings) -> &'a str {
    match key {
        "barge-in" => {
            if s.barge_in {
                "true"
            } else {
                "false"
            }
        }
        "speak" => {
            if s.speak {
                "true"
            } else {
                "false"
            }
        }
        "speak-progress" => {
            if s.speak_progress {
                "true"
            } else {
                "false"
            }
        }
        "routing.announce" => {
            if s.routing.announce {
                "true"
            } else {
                "false"
            }
        }
        "routing.auto-model" => {
            if s.routing.auto_model {
                "true"
            } else {
                "false"
            }
        }
        "chat" => s.chat.as_str(),
        "routing.router" => s.routing.router.as_str(),
        "routing.reasoning" => s.routing.reasoning.as_str(),
        "approvals.reviewer" => s.approvals.reviewer.as_str(),
        "stt.conversation" => s.stt.conversation.as_str(),
        "stt.engine" => s.stt.engine.as_str(),
        "stt.lazy" => {
            if s.stt.lazy {
                "true"
            } else {
                "false"
            }
        }
        _ => "",
    }
}
fn cycle_next(current: &str, options: &[&str]) -> String {
    let i = options.iter().position(|o| *o == current).unwrap_or(0);
    options[(i + 1) % options.len()].to_string()
}

impl Panel {
    pub fn display(&self, s: &Settings, connected: bool, models: &[Model]) -> String {
        if let Some(key) = self.field {
            return match key {
                "model" | "routing.plugin-model" | "routing.routine-model" => {
                    model_picker(key, s, models)
                }
                "agent" | "routing.coding" | "routing.routine" => {
                    let mut text = String::from(
                        "CLIs Accessor found on this machine (type a number or id):\n",
                    );
                    for (i, h) in crate::config::harness_offers(s, None).iter().enumerate() {
                        let mark = if h.found { "found" } else { "not found" };
                        text.push_str(&format!(
                            "{}  {} ({})  {} · {}\n",
                            i + 1,
                            h.name,
                            h.id,
                            mark,
                            h.detail
                        ));
                    }
                    text.push_str("Wake-word spotting is local. Esc returns.");
                    text
                }
                "routing.compaction-harness" => {
                    let mut text = String::from(
                        "Compaction provider (type a number or id). Only installed CLIs are listed:\n",
                    );
                    for (i, (id, name)) in compaction_providers(s).iter().enumerate() {
                        text.push_str(&format!("{}  {} ({})\n", i + 1, name, id));
                    }
                    text.push_str("Then pick a model from that list. Esc returns.");
                    text
                }
                "routing.compaction-model" => {
                    let catalog = compaction_models(&s.routing.compaction_harness, models);
                    format!(
                        "Compaction models for {} (type a number or id):\n{}\nEsc returns.",
                        s.routing.compaction_harness,
                        catalog
                            .iter()
                            .enumerate()
                            .map(|(i, m)| format!("{}  {} ({})", i + 1, m.name, m.id))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                }
                "routing.compact-tokens" => {
                    "Approximate token threshold (500–200000). Esc returns.".into()
                }
                "prompt" => format!(
                    "Voice metaprompt sent to real harnesses. Type default to restore.\n\n{}\n\nEsc returns.",
                    s.prompt
                ),
                "idle-seconds" => "Wake-listening seconds (0–3600; 0 = no timeout). Esc returns.".into(),
                "wake-code" => "Wake code (digits). Esc returns.".into(),
                "tts.provider" => "system, kokoro, cartesia, or off. Esc returns.".into(),
                "tts.local-voice" | "tts.voice" => "Voice ID. Esc returns.".into(),
                "tts.speed" => "Speed 0.6–1.5. Esc returns.".into(),
                "sounds.think" | "sounds.wake" | "sounds.sleep" => {
                    "Volume 0–1.5 (or 0–150%). 0 is silent, 1 is default. Esc returns.".into()
                }
                "stt.engine" => engine_picker(s),
                _ => format!("Enter a new value for {key}. Esc returns."),
            };
        }
        let list = rows(self.page, s, connected);
        let mut out = format!(
            "{}  ·  ↑/↓ move  ·  Enter  ·  Esc back\n",
            self.page.title().to_uppercase()
        );
        if self.page == Page::Home {
            out.push_str("Categories — not a numbered dump.\n");
        }
        if self.page == Page::Harnesses {
            out.push_str("CLIs on this machine:\n");
            for h in crate::config::harness_offers(s, None) {
                out.push_str(&format!(
                    "  {}  {} ({})  {}\n",
                    if h.found { "found  " } else { "missing" },
                    h.name,
                    h.id,
                    h.detail
                ));
            }
        }
        for (i, row) in list.iter().enumerate() {
            out.push_str(if i == self.cursor { "› " } else { "  " });
            out.push_str(&row.label);
            out.push('\n');
            if i == self.cursor {
                out.push_str("hint:  ");
                out.push_str(hint(&row.action));
                out.push('\n');
            }
        }
        out.push_str(
            "\nMicrophone is paused here. Esc goes up; Esc on the category list starts listening.",
        );
        out
    }
    pub fn nav(&mut self, dir: i32, s: &Settings, connected: bool) {
        if self.field.is_some() {
            return;
        }
        let n = rows(self.page, s, connected).len();
        if n == 0 {
            return;
        }
        let cur = self.cursor as i32 + dir;
        self.cursor = ((cur % n as i32 + n as i32) % n as i32) as usize;
    }
    pub fn back(&mut self) -> Answer {
        if self.field.is_some() {
            self.field = None;
            return Answer::Show;
        }
        if self.page == Page::Home {
            Answer::Close
        } else {
            self.page = Page::Home;
            self.cursor = 0;
            Answer::Show
        }
    }
    pub fn activate(&mut self, s: &Settings, _models: &[Model], connected: bool) -> Result<Answer> {
        if self.field.is_some() {
            return Ok(Answer::Show);
        }
        let list = rows(self.page, s, connected);
        let Some(row) = list.get(self.cursor) else {
            return Ok(Answer::Show);
        };
        match &row.action {
            Action::Open(page) => {
                self.page = *page;
                self.cursor = 0;
                Ok(Answer::Show)
            }
            Action::Close => Ok(Answer::Close),
            Action::Toggle(key) => {
                let next = if current_value(key, s) == "true" {
                    "false"
                } else {
                    "true"
                };
                Ok(Answer::Command(format!("/config set {key} {next}")))
            }
            Action::Cycle(key, options) => {
                let next = cycle_next(current_value(key, s), options);
                Ok(Answer::Command(format!("/config set {key} {next}")))
            }
            Action::Edit(key) => {
                self.field = Some(*key);
                Ok(Answer::Show)
            }
            Action::Run(cmd) => Ok(Answer::Command((*cmd).into())),
        }
    }
    pub fn answer(&mut self, text: &str, s: &Settings, models: &[Model]) -> Result<Answer> {
        let value = text.trim();
        if value.eq_ignore_ascii_case("up") {
            self.nav(-1, s, false);
            return Ok(Answer::Show);
        }
        if value.eq_ignore_ascii_case("down") {
            self.nav(1, s, false);
            return Ok(Answer::Show);
        }
        if let Some(key) = self.field {
            if value.is_empty() {
                return Ok(Answer::Show);
            }
            let value = match key {
                "agent" | "routing.coding" | "routing.routine" => match value {
                    "1" | "codex" => "codex".into(),
                    "2" | "claude" => "claude".into(),
                    "3" | "antigravity" | "agy" => "antigravity".into(),
                    "4" | "mock" => "mock".into(),
                    other => other.to_lowercase(),
                },
                "routing.compaction-harness" => {
                    let catalog = compaction_providers(s);
                    if let Ok(n) = value.parse::<usize>() {
                        catalog
                            .get(n.saturating_sub(1))
                            .map(|(id, _)| (*id).to_string())
                            .unwrap_or_else(|| value.to_lowercase())
                    } else if catalog.iter().any(|(id, _)| *id == value.to_lowercase()) {
                        value.to_lowercase()
                    } else {
                        anyhow::bail!("Choose a listed compaction provider.");
                    }
                }
                "model" | "routing.plugin-model" | "routing.routine-model" => {
                    let catalog = catalog_for_key(key, s, models);
                    resolve_model(value, &catalog).ok_or_else(|| {
                        anyhow::anyhow!(
                            "Choose a listed model for this harness, or 0 for the harness default."
                        )
                    })?
                }
                "routing.compaction-model" => {
                    let catalog = compaction_models(&s.routing.compaction_harness, models);
                    resolve_model(value, &catalog)
                        .or_else(|| {
                            if value.chars().all(|c| {
                                c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | '.' | '_')
                            }) {
                                Some(value.to_string())
                            } else {
                                None
                            }
                        })
                        .ok_or_else(|| {
                            anyhow::anyhow!("Choose a model from this compaction provider's list.")
                        })?
                }
                "stt.engine" => resolve_engine(value).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Choose a listed local STT model. Esc returns without downloading."
                    )
                })?,
                _ => value.to_owned(),
            };
            self.field = None;
            return Ok(Answer::Command(format!("/config set {key} {value}")));
        }
        match value {
            "0" | "close" | "esc" => Ok(self.back()),
            "enter" | "" => self.activate(s, models, false),
            _ => {
                if let Ok(n) = value.parse::<usize>() {
                    if n >= 1 {
                        self.cursor = n - 1;
                        return self.activate(s, models, false);
                    }
                }
                bail!("↑/↓ then Enter, or Esc. Or /settings KEY VALUE.")
            }
        }
    }
}
fn normalized(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
pub fn resolve_model(value: &str, models: &[Model]) -> Option<String> {
    if value == "0" || value.eq_ignore_ascii_case("default") {
        return Some("default".into());
    }
    if let Ok(index) = value.parse::<usize>() {
        return models.get(index.checked_sub(1)?).map(|m| m.id.clone());
    }
    let needle = normalized(value);
    let pick = |ok: Vec<&Model>| {
        if ok.len() == 1 {
            Some(ok[0].id.clone())
        } else {
            None
        }
    };
    if let Some(id) = pick(
        models
            .iter()
            .filter(|m| normalized(&m.id) == needle || normalized(&m.name) == needle)
            .collect(),
    ) {
        return Some(id);
    }
    if let Some(id) = pick(
        models
            .iter()
            .filter(|m| {
                m.name
                    .split_whitespace()
                    .last()
                    .is_some_and(|w| normalized(w) == needle)
            })
            .collect(),
    ) {
        return Some(id);
    }
    pick(
        models
            .iter()
            .filter(|m| {
                m.id.rsplit(['-', '/', '.'])
                    .next()
                    .is_some_and(|s| normalized(s) == needle)
            })
            .collect(),
    )
}
fn engine_label(s: &Settings) -> String {
    let Some(offer) = crate::stt_models::get(&s.stt.engine) else {
        return s.stt.engine.clone();
    };
    let status = s.assets().ok().map(|assets| {
        if crate::stt_models::installed(&assets, offer) {
            "installed".to_string()
        } else {
            format!(
                "download ~{}",
                crate::stt_models::size_label(crate::stt_models::missing_bytes(&assets, offer))
            )
        }
    });
    match status {
        Some(status) => format!("{} · {}", offer.name, status),
        None => offer.name.to_string(),
    }
}
fn engine_status(s: &Settings, offer: &crate::stt_models::Offer) -> String {
    match s.assets() {
        Ok(assets) if crate::stt_models::installed(&assets, offer) => "installed".into(),
        Ok(assets) => format!(
            "not downloaded · ~{}",
            crate::stt_models::size_label(crate::stt_models::missing_bytes(&assets, offer))
        ),
        Err(_) => "size shown after you pick it".into(),
    }
}
fn engine_picker(s: &Settings) -> String {
    let mut text = String::from(
        "Local STT models — type a number or id. Esc returns with no download.\nOnly a missing model you actually pick asks to download.\n",
    );
    for (i, offer) in crate::stt_models::OFFERS.iter().enumerate() {
        let current = if s.stt.engine == offer.id {
            "  current"
        } else {
            ""
        };
        text.push_str(&format!(
            "{}  {} ({})  {}{}\n",
            i + 1,
            offer.name,
            offer.id,
            engine_status(s, offer),
            current
        ));
    }
    text.push_str(
        "Canary is already installed. Skip Parakeet unless you want that ~640 MB upgrade.",
    );
    text
}
fn resolve_engine(value: &str) -> Option<String> {
    if let Ok(index) = value.parse::<usize>() {
        return crate::stt_models::OFFERS
            .get(index.checked_sub(1)?)
            .map(|o| o.id.to_string());
    }
    crate::stt_models::resolve(value).map(str::to_string)
}
fn named_harness(value: &str) -> Option<&'static str> {
    match value.trim() {
        "codex" => Some("codex"),
        "claude" => Some("claude"),
        "antigravity" | "agy" => Some("antigravity"),
        "mock" => Some("mock"),
        _ => None,
    }
}
fn extract_harness_request(
    text: &str,
    settings: Option<&Settings>,
) -> Option<Result<(String, String)>> {
    let wanted = if let Some(value) = text
        .strip_prefix("switch agent to ")
        .or_else(|| text.strip_prefix("use agent "))
        .or_else(|| text.strip_prefix("switch to "))
        .or_else(|| text.strip_prefix("switch over to "))
    {
        value.trim().trim_start_matches("the ").to_string()
    } else if text.contains("switch") {
        let hay = text.replace(' ', "");
        ["antigravity", "codex", "claude", "mock", "agy"]
            .into_iter()
            .find(|n| hay.contains(n))
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };
    if wanted.is_empty() {
        return None;
    }
    if let Some(s) = settings {
        let role = wanted.trim_end_matches(" harness").trim_end_matches(" cli");
        if role == "plugin" || role == "plugins" {
            return Some(Ok(("agent".into(), s.agent.clone())));
        }
        if role == "coding" || role == "code" {
            return Some(Ok(("agent".into(), s.routing.coding.clone())));
        }
        if role == "everyday" || role == "routine" {
            return Some(Ok(("agent".into(), s.routing.routine.clone())));
        }
    }
    let Some(id) = named_harness(&wanted) else {
        if text.contains("switch agent") || text.starts_with("switch to ") {
            return Some(Err(anyhow::anyhow!(
                "Unknown harness. Use Codex, Claude, Antigravity, or Mock."
            )));
        }
        return None;
    };
    Some(Ok(("agent".into(), id.into())))
}
pub fn voice_command(
    text: &str,
    models: &[Model],
    settings: Option<&Settings>,
) -> Option<Result<(String, String)>> {
    let text = text.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    if let Some(value) = text
        .strip_prefix("switch model to ")
        .or_else(|| text.strip_prefix("use model "))
    {
        let catalog = if let Some(s) = settings {
            let mut list = harness_models(&s.routing.coding, models);
            for extra in [
                harness_models(&s.agent, models),
                harness_models(&s.routing.routine, models),
            ] {
                for m in extra {
                    if !list.iter().any(|x| x.id == m.id) {
                        list.push(m);
                    }
                }
            }
            list
        } else {
            models.to_vec()
        };
        return Some(resolve_model(value, &catalog).map(|v|("model".into(),v)).ok_or_else(||anyhow::anyhow!("That model is not in the available list. Open /settings to select or refresh models.")));
    }
    if let Some(value) = extract_harness_request(&text, settings) {
        return Some(value);
    }
    for level in ["default", "low", "medium", "high"] {
        if text == format!("use {level} reasoning")
            || text == format!("use {level} effort")
            || text == format!("switch reasoning to {level}")
            || text == format!("set reasoning to {level}")
        {
            return Some(Ok(("routing.reasoning".into(), level.into())));
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voice_switches_only_known_models() {
        let models = vec![Model {
            id: "test-astra".into(),
            name: "Test Astra".into(),
        }];
        assert_eq!(
            voice_command("Switch model to Astra.", &models, None)
                .unwrap()
                .unwrap()
                .1,
            "test-astra"
        );
        assert!(voice_command("switch model to imaginary", &models, None)
            .unwrap()
            .is_err());
        assert!(voice_command("Tell me about switching models", &models, None).is_none());
        assert!(
            voice_command("switch agent to claude", &models, None)
                .unwrap()
                .unwrap()
                .1
                == "claude"
        );
        assert_eq!(
            voice_command("Cool, can you switch to Codex?", &models, None)
                .unwrap()
                .unwrap()
                .1,
            "codex"
        );
        assert_eq!(
            voice_command("use high reasoning", &models, None)
                .unwrap()
                .unwrap(),
            ("routing.reasoning".into(), "high".into())
        );
        let mixed = vec![
            Model {
                id: "fixture-sol".into(),
                name: "Fixture Sol".into(),
            },
            Model {
                id: "gpt-5.4-sol".into(),
                name: "gpt-5.4-sol".into(),
            },
        ];
        assert_eq!(resolve_model("Sol", &mixed).as_deref(), Some("fixture-sol"));
    }
    #[test]
    fn arrows_open_voice_with_mandatory_wake() {
        let s = Settings::default();
        let mut p = Panel::default();
        assert!(p.display(&s, false, &[]).contains("Voice"));
        let Answer::Show = p.answer("enter", &s, &[]).unwrap() else {
            panic!("open voice");
        };
        let voice = p.display(&s, false, &[]);
        assert!(voice.contains("Wake code"));
        assert!(!voice.contains("Wake mode"));
        p.nav(1, &s, false);
        let Answer::Show = p.activate(&s, &[], false).unwrap() else {
            panic!("edit idle timeout");
        };
        assert!(p.display(&s, false, &[]).contains("Wake-listening seconds"));
        let mut home = Panel::default();
        home.answer("3", &s, &[]).unwrap();
        let harnesses = home.display(&s, false, &[]);
        assert!(harnesses.contains("CLIs on this machine"));
        assert!(harnesses.contains("Mock"));
        home.answer("enter", &s, &[]).unwrap();
        let picker = home.display(&s, false, &[]);
        assert!(picker.contains("CLIs Accessor found on this machine"));
        assert!(picker.contains("mock"));
        let mut speech = Panel::default();
        speech.answer("2", &s, &[]).unwrap();
        let Answer::Show = speech.answer("6", &s, &[]).unwrap() else {
            panic!("open local STT list");
        };
        let list = speech.display(&s, false, &[]);
        assert!(list.contains("Parakeet"));
        assert!(list.contains("whisper-tiny"));
        assert!(list.contains("Esc returns with no download"));
        let Answer::Show = speech.back() else {
            panic!("esc list");
        };
        assert!(speech.display(&s, false, &[]).contains("Canary 180M Flash"));
    }
    #[test]
    fn everyday_antigravity_lists_gemini_not_codex_catalog() {
        let s = Settings {
            routing: crate::config::Routing {
                routine: "antigravity".into(),
                ..crate::config::Routing::default()
            },
            ..Settings::default()
        };
        let text = model_picker("routing.routine-model", &s, &[]);
        assert!(text.contains("Everyday model for antigravity"));
        assert!(text.contains("gemini-3.8-flash"));
        assert!(!text.contains("fixture-astra"));
        let chosen = catalog_for_key("routing.routine-model", &s, &[]);
        assert_eq!(
            resolve_model("1", &chosen).as_deref(),
            Some("gemini-3.8-flash")
        );
    }
}
