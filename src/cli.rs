use crate::{
    app, audio,
    config::{self, Settings},
    connectors, speech, ui,
};
use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use dialoguer::{Input, Select};
use std::{ffi::OsString, path::PathBuf, time::Instant};

#[derive(Parser)]
#[command(
    name = "acc",
    version,
    about = "Accessor — local voice, your agents, your tools",
    after_help = "Quick start: acc -wakecode 29 speak\nSetup: acc setup · acc tts setup · acc connectors setup"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Receive notifications from an existing email automation; no inbox polling.
    Events {
        #[command(subcommand)]
        action: EventCommand,
    },
    /// Manage local notes, alarms, and persistent scheduled tasks.
    Organizer {
        #[command(subcommand)]
        action: OrganizerCommand,
    },
    /// Start the voice gateway (also the default when no command is given).
    Run(Run),
    /// Save wake, voice, and speech-file settings interactively.
    Setup,
    /// Inspect or update saved settings. Secrets are never printed.
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Check local dependencies without opening the microphone.
    Doctor,
    /// Check Codex / Claude Code / Antigravity CLI versions and apply updates.
    Update {
        /// Print versions only; do not install.
        #[arg(long)]
        check: bool,
    },
    /// List available microphones.
    Devices,
    /// Test local speech recognition without contacting an agent.
    Stt {
        #[command(subcommand)]
        action: SttCommand,
    },
    /// Transcribe a 16 kHz mono WAV locally (alias for stt test --file).
    Transcribe {
        file: PathBuf,
        #[arg(long)]
        model_dir: Option<PathBuf>,
    },
    /// Configure, audition, and benchmark spoken replies.
    Tts {
        #[command(subcommand)]
        action: TtsCommand,
    },
    /// Reuse the agent's plugins and MCP connections, including Gmail.
    Connectors {
        #[command(subcommand)]
        action: ConnectorCommand,
    },
    /// Open the selected Codex installation directly for login or setup.
    Agent {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Save a TypeSafe API key for Jev routing.
    Jev {
        #[command(subcommand)]
        action: JevCommand,
    },
}
#[derive(Clone, Copy, ValueEnum, PartialEq)]
pub enum Backend {
    Mock,
    Codex,
}
#[derive(Parser)]
pub struct Run {
    /// Handle locally queued Gmail events, even while voice access is asleep.
    #[arg(long)]
    pub events: bool,
    #[arg(long)]
    pub wake_code: Option<String>,
    #[arg(long)]
    pub wake_alias: Vec<String>,
    #[arg(long, value_enum)]
    pub agent: Option<Backend>,
    /// Type simulated transcripts; never opens the microphone.
    #[arg(long)]
    pub text: bool,
    /// Seconds to wait for speech after a bare wake code; 0 waits indefinitely.
    #[arg(long,value_parser=clap::value_parser!(u64).range(0..=3600))]
    pub idle_seconds: Option<u64>,
    #[arg(long)]
    pub model_dir: Option<PathBuf>,
    #[arg(long)]
    pub microphone: Option<String>,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    #[arg(long)]
    pub workspace_write: bool,
    #[arg(long)]
    pub codex_bin: Option<PathBuf>,
    #[arg(long, conflicts_with = "no_speak")]
    pub speak: bool,
    #[arg(long)]
    pub no_speak: bool,
    #[arg(long,value_parser=["system","kokoro","cartesia","off"])]
    pub tts: Option<String>,
    #[arg(long)]
    pub no_chime: bool,
    /// Use ordinary terminal lines instead of the dashboard.
    #[arg(long)]
    pub plain: bool,
    #[arg(skip)]
    pub stt_test: bool,
}
#[derive(Subcommand)]
enum ConfigCommand {
    Show,
    Path,
    /// Print the settings folder and what to copy to another machine.
    Locations,
    Set {
        key: String,
        value: String,
    },
}
#[derive(Subcommand)]
enum SttCommand {
    Test {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        model_dir: Option<PathBuf>,
        #[arg(long)]
        microphone: Option<String>,
        #[arg(long)]
        plain: bool,
    },
}
#[derive(Subcommand)]
enum TtsCommand {
    Setup,
    /// Speak a sample; --output writes neural speech to WAV without playback.
    Test {
        #[arg(default_value = "Hello, I'm Accessor. I'm listening, and ready to help.")]
        text: String,
        #[arg(long,value_parser=["system","kokoro","cartesia"])]
        provider: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Voices {
        #[arg(long,value_parser=["kokoro","cartesia"])]
        provider: Option<String>,
    },
    /// Remove the stored Cartesia key.
    ForgetKey,
}
#[derive(Subcommand)]
enum JevCommand {
    /// Save a TypeSafe API key in the OS credential store.
    Key,
    /// Remove the stored TypeSafe key.
    ForgetKey,
}
#[derive(Subcommand)]
enum ConnectorCommand {
    List,
    Status,
    Setup,
}
#[derive(Subcommand)]
enum EventCommand {
    /// Save the email address allowed to receive event-triggered replies.
    Setup,
    /// Submit Gmail API IDs from a trusted local automation (no email body).
    Emit {
        #[arg(long)]
        thread_id: String,
        #[arg(long)]
        message_id: String,
    },
    Status,
}

#[derive(Subcommand)]
enum OrganizerCommand {
    /// Show the notes folder and pending alarms/tasks.
    Status,
    /// Save a Markdown note.
    Note {
        text: String,
        #[arg(long)]
        title: Option<String>,
    },
    /// Set an alarm relative to now.
    Alarm {
        #[arg(long)]
        in_seconds: u64,
        #[arg(long)]
        label: Option<String>,
    },
    /// Schedule an agent prompt relative to now.
    Task {
        prompt: String,
        #[arg(long)]
        in_seconds: u64,
        #[arg(long)]
        every_seconds: Option<u64>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        harness: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// Cancel a pending item by its 8-character ID, or use "all".
    Cancel { id: String },
}

fn normalized(mut args: Vec<OsString>) -> Vec<OsString> {
    if args.len() == 1 {
        args.push("run".into());
        return args;
    }
    let first = args[1].to_string_lossy();
    if first == "speak"
        || (first.starts_with('-')
            && !["--help", "-h", "--version", "-V"].contains(&first.as_ref()))
    {
        args.insert(1, "run".into());
    }
    if args[1] == "run" {
        // Only a standalone final `speak` is an alias; values named speak remain values.
        let mut expects_value = false;
        for arg in args.iter_mut().skip(2) {
            if expects_value {
                expects_value = false;
                continue;
            }
            if arg == "-wakecode" || arg == "--wakecode" {
                *arg = "--wake-code".into();
            }
            if arg == "speak" {
                *arg = "--speak".into();
            }
            expects_value = [
                "--wake-code",
                "--wake-alias",
                "--agent",
                "--idle-seconds",
                "--model-dir",
                "--microphone",
                "--workspace",
                "--codex-bin",
                "--tts",
            ]
            .iter()
            .any(|s| arg == s);
        }
    }
    args
}
pub async fn entry() -> Result<()> {
    let cli = Cli::parse_from(normalized(std::env::args_os().collect()));
    match cli.command {
        Commands::Organizer { action } => match action {
            OrganizerCommand::Status => {
                println!("{}", crate::organizer::list()?);
                Ok(())
            }
            OrganizerCommand::Note { text, title } => {
                let path = crate::organizer::add_note(&text, title.as_deref())?;
                println!("Saved {}", path.display());
                Ok(())
            }
            OrganizerCommand::Alarm { in_seconds, label } => {
                let item = crate::organizer::add_alarm(label.as_deref(), Some(in_seconds), None)?;
                println!("Alarm {} saved for Unix {}.", item.id, item.at_unix);
                Ok(())
            }
            OrganizerCommand::Task {
                prompt,
                in_seconds,
                every_seconds,
                label,
                harness,
                model,
            } => {
                let item = crate::organizer::add_task(
                    &prompt,
                    label.as_deref(),
                    Some(in_seconds),
                    None,
                    every_seconds,
                    harness.as_deref(),
                    model.as_deref(),
                )?;
                println!("Task {} saved for Unix {}.", item.id, item.next_unix);
                Ok(())
            }
            OrganizerCommand::Cancel { id } => {
                crate::organizer::require_id(&id)?;
                println!(
                    "{}",
                    if crate::organizer::cancel(&id)? {
                        "Cancelled."
                    } else {
                        "No matching pending item."
                    }
                );
                Ok(())
            }
        },
        Commands::Events { action } => match action {
            EventCommand::Setup => {
                let mut s = Settings::load()?;
                s.event_owner = Some(
                    Input::<String>::new()
                        .with_prompt("Your email address for replies")
                        .with_initial_text(s.event_owner.unwrap_or_default())
                        .interact_text()?,
                );
                s.save()?;
                println!("Saved. Configure your trusted email automation to call:\nacc events emit --thread-id THREAD_ID --message-id MESSAGE_ID\nThen run acc --events. The automation must authenticate the sender before emitting events.");
                Ok(())
            }
            EventCommand::Emit {
                thread_id,
                message_id,
            } => {
                crate::triggers::emit(crate::triggers::Event {
                    thread_id,
                    message_id,
                })?;
                println!("Notification queued. An instance started with --events will handle it.");
                Ok(())
            }
            EventCommand::Status => crate::triggers::status(),
        },
        Commands::Run(args) => app::run(args).await,
        Commands::Devices => audio::devices(),
        Commands::Setup => setup().await,
        Commands::Config { action } => match action {
            ConfigCommand::Show => {
                println!("{}", serde_json::to_string_pretty(&Settings::load()?)?);
                Ok(())
            }
            ConfigCommand::Path => {
                println!("{}", config::path()?.display());
                Ok(())
            }
            ConfigCommand::Locations => {
                println!("{}", config::locations(&Settings::load()?));
                Ok(())
            }
            ConfigCommand::Set { key, value } => Settings::load()?.set(&key, &value),
        },
        Commands::Doctor => doctor().await,
        Commands::Update { check } => {
            let text = crate::updates::report(&Settings::load()?, !check).await?;
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        Commands::Transcribe { file, model_dir } => transcribe(file, model_dir).await,
        Commands::Stt {
            action:
                SttCommand::Test {
                    file,
                    model_dir,
                    microphone,
                    plain,
                },
        } => {
            if let Some(file) = file {
                transcribe(file, model_dir).await
            } else {
                println!("Live transcription test: ALL speech is displayed. Nothing is sent to an agent. Ctrl+C to stop.");
                let mut args = Run::parse_from(["acc"]);
                args.stt_test = true;
                args.no_speak = true;
                args.no_chime = true;
                args.plain = plain;
                args.model_dir = model_dir;
                args.microphone = microphone;
                app::run(args).await
            }
        }
        Commands::Tts { action } => match action {
            TtsCommand::Setup => tts_setup(),
            TtsCommand::ForgetKey => config::delete_secret("cartesia"),
            TtsCommand::Voices { provider } => {
                let s = Settings::load()?;
                if provider.as_deref().unwrap_or(&s.tts.provider) == "cartesia" {
                    speech::voices().await
                } else {
                    speech::local_voices(&s).await
                }
            }
            TtsCommand::Test {
                text,
                provider,
                output,
            } => {
                let mut s = Settings::load()?;
                if let Some(p) = provider {
                    s.tts.provider = p;
                }
                s.validate()?;
                let start = Instant::now();
                if let Some(path) = output {
                    ensure!(
                        s.tts.provider != "off",
                        "Select a voice provider before exporting speech"
                    );
                    let bytes = speech::render(&speech::spoken_text(&text), &s.tts).await?;
                    let (rate, samples) = crate::audio::decode_wav(&bytes)?;
                    let seconds = samples.len() as f64 / rate as f64;
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .context("Output file must not already exist")?;
                    std::io::Write::write_all(&mut file, &bytes)?;
                    println!(
                        "Saved {:.2}s audio in {:.2}s to {}",
                        seconds,
                        start.elapsed().as_secs_f64(),
                        path.display()
                    );
                } else {
                    let mut job = speech::start(text, s.tts);
                    (&mut job.task).await??;
                    println!(
                        "Speech test finished in {:.2}s (including playback)",
                        start.elapsed().as_secs_f64()
                    );
                }
                Ok(())
            }
        },
        Commands::Connectors { action } => match action {
            ConnectorCommand::List => connectors::delegate(&["plugin".into(), "list".into()]).await,
            ConnectorCommand::Status => connectors::status().await,
            ConnectorCommand::Setup => {
                println!("Gmail, Calendar, Drive, Slack, and other integrations belong to your agent.\nIn Codex: enter /plugins, choose the plugin, and complete its connection flow.\nFor an MCP server: acc agent mcp add ...; acc agent mcp login NAME.\nRestart Accessor after changing agent integrations.\n\nOpening Codex now. Accessor does not store your connector credentials.");
                connectors::delegate(&[]).await
            }
        },
        Commands::Agent { args } => connectors::delegate(&args).await,
        Commands::Jev { action } => match action {
            JevCommand::Key => {
                let key = read_secret_line(
                    "TypeSafe API key (saved in the OS credential store, not settings.json)",
                )?;
                config::save_secret("typesafe", &key)?;
                println!("Saved. In Accessor: Harnesses → Jev auto-select on, or Router → jev.");
                Ok(())
            }
            JevCommand::ForgetKey => config::delete_secret("typesafe"),
        },
    }
}
async fn transcribe(file: PathBuf, dir: Option<PathBuf>) -> Result<()> {
    let s = Settings::load()?;
    let assets = s.assets()?;
    if dir.is_none() {
        crate::stt_models::ensure_ready(&assets, "canary", |msg| println!("{msg}")).await?;
    }
    let dir = dir.unwrap_or(assets.join("models/canary-180m-flash"));
    let start = Instant::now();
    let mut model = audio::load_model(&dir)?;
    println!("Model loaded in {:.2}s", start.elapsed().as_secs_f64());
    let samples = transcribe_rs::audio::read_wav_samples(&file)?;
    let start = Instant::now();
    let text = audio::transcribe(&mut model, &samples)?;
    println!(
        "{}\n{:.2}s audio, {:.2}s inference",
        ui::safe(&text),
        samples.len() as f64 / 16000.,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
async fn setup() -> Result<()> {
    let mut s = Settings::load()?;
    s.wake_code = Input::new()
        .with_prompt("Wake code (activation phrase)")
        .default(s.wake_code)
        .interact_text()?;
    s.idle_seconds = Input::new()
        .with_prompt("Idle seconds before sleep (0 = never)")
        .default(s.idle_seconds)
        .interact_text()?;
    s.speak = dialoguer::Confirm::new()
        .with_prompt("Speak agent replies?")
        .default(s.speak)
        .interact()?;
    s.assets_dir = Some(s.assets()?);
    s.save()?;
    let assets = s.assets()?;
    crate::stt_models::ensure_ready(&assets, &s.stt.engine, |msg| println!("{msg}")).await?;
    println!(
        "Saved to {}. Speech files: {}. Next: acc tts setup; acc connectors setup; acc",
        config::path()?.display(),
        assets.display()
    );
    Ok(())
}
fn tts_setup() -> Result<()> {
    let mut s = Settings::load()?;
    let options = [
        "System voice — installed on this computer",
        "Kokoro — local neural speech",
        "Cartesia — cloud speech (API key required)",
        "Off",
    ];
    let n = Select::new()
        .with_prompt("Spoken replies")
        .items(options)
        .default(0)
        .interact()?;
    s.tts.provider = ["system", "kokoro", "cartesia", "off"][n].into();
    if n == 1 {
        s.tts.local_voice = Input::new()
            .with_prompt("Kokoro voice")
            .default(s.tts.local_voice)
            .interact_text()?;
        println!("One-time local install: python scripts/setup_tts.py\nVoice list: acc tts voices --provider kokoro");
    } else if n == 2 {
        let key = read_secret_line("Cartesia API key (saved in the OS credential store)")?;
        config::save_secret("cartesia", &key)?;
        s.tts.voice = Input::new()
            .with_prompt("Cartesia voice ID")
            .default(s.tts.voice)
            .interact_text()?;
        s.tts.model = Input::new()
            .with_prompt("Cartesia model")
            .default(s.tts.model)
            .interact_text()?;
    }
    s.assets_dir = Some(s.assets()?);
    s.save()?;
    println!("Saved. Try acc tts test.");
    Ok(())
}
fn read_secret_line(prompt: &str) -> Result<String> {
    use std::io::{self, IsTerminal, Read, Write};
    if !io::stdin().is_terminal() {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        let key = buf
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("");
        ensure!(!key.is_empty(), "No key on stdin");
        return Ok(key.into());
    }
    println!("{prompt}");
    println!("Paste the key, then Enter. Ctrl+Shift+V / Shift+Insert work in most terminals.");
    print!("key: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let key = line.trim();
    ensure!(!key.is_empty(), "Empty key");
    Ok(key.into())
}
async fn doctor() -> Result<()> {
    let mut s = Settings::load()?;
    let assets = s.assets()?;
    println!(
        "Accessor {}\nSettings: {}\nSpeech files: {}\nCodex: {}\nTTS: {}\nLocal STT engine: {}",
        env!("CARGO_PKG_VERSION"),
        config::path()?.display(),
        assets.display(),
        config::codex(&s).display(),
        s.tts.provider,
        s.stt.engine
    );
    if !crate::stt_models::speech_ready(&assets, &s.stt.engine) {
        println!("Installing local speech for this computer...");
        crate::stt_models::ensure_ready(&assets, &s.stt.engine, |msg| println!("  {msg}")).await?;
        if s.assets_dir.is_none() && std::env::var_os("ACC_ASSETS").is_none() {
            s.assets_dir = Some(assets.clone());
            s.save()?;
        }
    }
    for offer in crate::stt_models::OFFERS {
        let state = if crate::stt_models::installed(&assets, offer) {
            "installed".to_string()
        } else if offer.id == s.stt.engine {
            "download failed".into()
        } else {
            format!(
                "not downloaded (~{}) — pick it in /settings to fetch",
                crate::stt_models::size_label(crate::stt_models::missing_bytes(&assets, offer))
            )
        };
        println!("  {} ({}): {}", offer.name, offer.id, state);
    }
    let runtime = crate::stt_models::runtime_path(&assets);
    println!(
        "ONNX runtime: {}",
        if runtime.is_file() {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        "Kokoro: {}",
        if speech::local_python(&s)?.is_file() {
            "installed (use acc tts test to verify)"
        } else {
            "optional; run python scripts/setup_tts.py"
        }
    );
    if s.tts.provider == "cartesia" {
        println!(
            "Cartesia credential: {}",
            if config::secret("cartesia", "CARTESIA_API_KEY").is_ok() {
                "available"
            } else {
                "missing"
            }
        );
    }
    println!(
        "TypeSafe / Jev: {}",
        if config::optional_secret("typesafe", "TYPESAFE_API_KEY").is_some() {
            "credential available"
        } else {
            "missing — acc jev key, or set TYPESAFE_API_KEY"
        }
    );
    println!("Agent integrations: acc connectors status\nMicrophones: acc devices\nHarness updates: acc update   (acc update --check for versions only)\nNo microphone was opened and no provider request was sent.");
    println!("\n{}", config::locations(&s));
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shorthand_preserves_values() {
        let args = normalized(
            ["acc", "-wakecode", "29", "--workspace", "speak", "speak"]
                .into_iter()
                .map(Into::into)
                .collect(),
        );
        let cli = Cli::try_parse_from(args).unwrap();
        let Commands::Run(r) = cli.command else {
            panic!()
        };
        assert!(r.speak);
        assert_eq!(r.workspace, PathBuf::from("speak"));
        assert_eq!(r.wake_code.as_deref(), Some("29"));
    }
    #[test]
    fn management_help_stays_root() {
        assert_eq!(normalized(vec!["acc".into(), "--help".into()])[1], "--help");
    }
}
