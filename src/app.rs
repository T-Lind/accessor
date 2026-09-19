use crate::cli::{Backend, Run};
use crate::session::{Action, Session};
use crate::ui::{safe, Kind, Ui};
use crate::{agent, audio, config, speech, wake, Input};
use anyhow::{bail, Context, Result};
use std::{
    collections::{HashMap, VecDeque},
    io::{self, BufRead},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

struct LiveAgent {
    tx: mpsc::Sender<agent::CommandMessage>,
    task: tokio::task::JoinHandle<()>,
    first: Option<String>,
}

fn identity_status(harness: &str, model: Option<&str>) -> String {
    let shown = model
        .or_else(|| crate::config::harness_default_model(harness))
        .unwrap_or("default");
    format!(
        "{} · {} · {}",
        crate::identity::code(harness, Some(shown).filter(|m| *m != "default")),
        harness,
        shown
    )
}

struct BannerState {
    active: bool,
    busy: bool,
    approval: bool,
    speaking: bool,
    settings_open: bool,
    setup: bool,
    muted: bool,
}

fn banner(state: BannerState, identity: &str) -> String {
    let voice = if state.settings_open {
        "SETTINGS: mic paused — Esc until Activity, then say 29"
    } else if state.setup {
        "SETUP: microphone paused"
    } else if state.speaking {
        if state.muted {
            "SPEAKING: mic suppressed"
        } else {
            "SPEAKING: listening for interruptions"
        }
    } else if state.muted {
        "MUTED: audio discarded"
    } else if state.busy {
        "BLUE: waiting for reply"
    } else if state.active {
        "GREEN: conversation open"
    } else {
        "WHITE: waiting for wake code"
    };
    let task = if state.approval {
        "AMBER: approval pending"
    } else if state.busy {
        "BLUE: agent working"
    } else {
        "idle"
    };
    format!("{voice} | {task} | {identity}")
}

pub async fn run(mut args: Run) -> Result<()> {
    let mut settings = config::Settings::load()?;
    if let Some(backend) = args.agent {
        let name = if backend == Backend::Mock {
            "mock"
        } else {
            "codex"
        };
        settings.agent = name.into();
        settings.routing.coding = name.into();
        settings.routing.routine = name.into();
    }
    settings.addressed |= args.addressed;
    let mut models = crate::connectors::cached_models();
    let mut model_lookup: Option<tokio::task::JoinHandle<Result<Vec<crate::connectors::Model>>>> =
        None;
    let mut panel: Option<crate::settings_ui::Panel> = None;
    let mut connected = false;
    let mut pending_prompt: Option<String> = None;
    let mut pending_setting: Option<(String, String, bool)> = None;
    let mut pending_handoff: Option<(String, String)> = None;
    let mut session_pin: Option<(String, Option<String>)> = None;
    let mut pending_stt: Option<String> = None;
    let mut stt_download: Option<tokio::task::JoinHandle<Result<(String, String)>>> = None;
    let mut echo_guard = crate::echo::TextGuard::new();
    if let Some(provider) = &args.tts {
        settings.tts.provider = provider.clone();
        settings.validate()?;
    }
    let mut event_queue = if args.events {
        anyhow::ensure!(
            settings.event_owner.is_some(),
            "Run acc events setup before enabling event-triggered replies"
        );
        Some(crate::triggers::Queue::open()?)
    } else {
        None
    };
    let mut current_event: Option<crate::triggers::Event> = None;
    let mut event_cancelled = false;
    let mut waiting_event: Option<crate::triggers::Event> = None;
    let mut last_event_poll = Instant::now();
    let wake_code = args
        .wake_code
        .take()
        .unwrap_or_else(|| settings.wake_code.clone());
    let idle = args.idle_seconds.unwrap_or(settings.idle_seconds);
    settings.wake_code = wake_code.clone();
    settings.idle_seconds = idle;
    let assets = match args.model_dir.take() {
        Some(dir) if dir.join("models").is_dir() => dir,
        Some(dir) if dir.ends_with("canary-180m-flash") => dir
            .parent()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf)
            .unwrap_or(dir),
        Some(dir) => dir,
        None => settings.assets()?,
    };
    let mut speak = !args.no_speak && (args.speak || (!args.text && settings.speak));
    settings.speak = speak;
    let cues = !args.no_chime && !args.text;
    let mut ui = Ui::new(args.plain)?;
    ui.set_chat(&settings.chat);
    if !args.text {
        ui.message("Loading local speech...");
    }
    ui.draw()?;
    let wake = wake::WakeCode::new(&wake_code, &args.wake_alias)?;
    let workspace = args
        .workspace
        .canonicalize()
        .context("Workspace must exist")?;
    let mut session = Session::new(wake, settings.addressed, Duration::from_secs(idle));
    let epoch = Arc::new(AtomicU64::new(0));
    let muted = Arc::new(AtomicBool::new(false));
    let awake = Arc::new(AtomicBool::new(false));
    let cloud_stt = Arc::new(AtomicBool::new(settings.stt.conversation == "cartesia"));
    let lazy_stt = Arc::new(AtomicBool::new(settings.stt.lazy));
    let stt_engine = Arc::new(Mutex::new(settings.stt.engine.clone()));
    let (input_tx, mut input_rx) = mpsc::channel(16);
    let mut microphone_available = args.text;
    let _capture = if args.text {
        None
    } else {
        match audio::listen(
            &assets,
            args.microphone
                .as_deref()
                .or(settings.microphone.as_deref()),
            input_tx.clone(),
            audio::MicFlags {
                epoch: epoch.clone(),
                muted: muted.clone(),
                awake: awake.clone(),
                cloud_stt: cloud_stt.clone(),
                lazy: lazy_stt.clone(),
                engine: stt_engine.clone(),
            },
        ) {
            Ok(capture) => {
                microphone_available = true;
                Some(capture)
            }
            Err(e) => {
                ui.message(format!("Microphone unavailable: {e:#}\nYou can still type messages or use /setup, /devices and /tts."));
                None
            }
        }
    };
    if !ui.interactive() {
        let input_tx = input_tx.clone();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(text) => {
                        if input_tx.blocking_send(Input::Text(text)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = input_tx.blocking_send(Input::Error(e.to_string()));
                        return;
                    }
                }
            }
            let _ = input_tx.blocking_send(Input::Eof);
        });
    }
    let (event_tx, mut events) = mpsc::channel(32);
    let mut agents: HashMap<String, LiveAgent> = HashMap::new();
    let mut agent_tx: Option<mpsc::Sender<agent::CommandMessage>> = None;
    let mut active_harness = settings.agent.clone();
    let mut active_model = settings.model.clone();
    let mut announce_next = false;
    let mut last_identity: Option<Instant> = None;
    let mut bridged: HashMap<String, bool> = HashMap::new();
    let mut transcript: VecDeque<(String, String)> = VecDeque::new();
    let mut busy = false;
    let mut approval = false;
    let mut user_muted = !microphone_available;
    muted.store(user_muted, Ordering::SeqCst);
    let mut eof = false;
    let mut silence_reply = false;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let mut speaker: Option<speech::Job> = None;
    let mut think: Option<audio::Cue> = None;
    let mut think_warned = false;
    let mut pause_until: Option<Instant> = None;
    ui.message(format!(
        "Accessor {} — {}",
        env!("CARGO_PKG_VERSION"),
        if args.text {
            "typed transcript mode (microphone off)"
        } else {
            "microphone on; transcription stays local"
        }
    ));
    ui.message("Type / for commands, /settings to configure, /tts to choose a voice, or a message to talk to the agent.");
    ui.message("Approvals only accept typed commands. Ambient transcripts are neither displayed nor saved (except explicit STT test mode).");
    ui.status(&banner(
        BannerState {
            active: session.active(),
            busy,
            approval,
            speaking: speaker.is_some(),
            settings_open: false,
            setup: false,
            muted: user_muted,
        },
        &identity_status(&active_harness, active_model.as_deref()),
    ));
    let mut wizard: Option<crate::dashboard::Wizard> = None;
    let mut pending_secret: Option<&'static str> = None;
    let mut utility: Option<tokio::task::JoinHandle<Result<String>>> = None;
    let mut speech_queue = VecDeque::<String>::new();
    loop {
        awake.store(session.active(), Ordering::SeqCst);
        ui.status(&banner(
            BannerState {
                active: session.active(),
                busy,
                approval,
                speaking: speaker.is_some(),
                settings_open: panel.is_some(),
                setup: pending_secret.is_some() || wizard.is_some(),
                muted: user_muted
                    || pending_secret.is_some()
                    || wizard.is_some()
                    || panel.is_some()
                    || (speaker.is_some() && !settings.barge_in),
            },
            &identity_status(&active_harness, active_model.as_deref()),
        ));
        ui.draw()?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            input = input_rx.recv(), if !eof || !input_rx.is_empty() => {
                let Some(input) = input else { break; };
                let is_event=matches!(&input,Input::Trigger(_));
                let spoken_setting=matches!(&input,Input::Configure{spoken:true,..});
                let (mut text, typed) = match input {
                    Input::Configure{key,value,..}=>(format!("/config set {key} {value}"),true),
                    Input::Trigger(event)=>{
                        if busy {waiting_event=Some(event);continue;}
                        let prompt=event.prompt(settings.event_owner.as_deref().unwrap_or(""));
                        event_cancelled=false;current_event=Some(event);(prompt,false)
                    }
                    Input::Eof => { if args.text { eof=true; } continue; }
                    Input::Activity { epoch: e } => {
                        if e==epoch.load(Ordering::SeqCst) && !muted.load(Ordering::SeqCst) {
                            session.touch(Instant::now());
                            if settings.barge_in {
                                if think.is_some() && (busy || session.active()) {
                                    think=None;
                                }
                                if !settings.addressed && session.active() {
                                    if let Some(job)=&speaker {job.paused.store(true,Ordering::SeqCst);pause_until=Some(Instant::now()+Duration::from_secs(2));}
                                }
                            }
                        }
                        continue;
                    }
                    Input::Error(e) => {
                        let text = safe(&e);
                        if text.contains("kept listening") {
                            ui.message(format!("Audio: {text}"));
                            continue;
                        }
                        bail!("Audio/input stopped: {text}");
                    }
                    Input::Voice { text, epoch: captured_epoch } => {
                        if muted.load(Ordering::SeqCst) || captured_epoch != epoch.load(Ordering::SeqCst) { continue; }
                        if echo_guard.matches(&text,speaker.is_some()) {
                            if let Some(job)=&speaker {job.paused.store(false,Ordering::SeqCst);}pause_until=None;continue;
                        }
                        crate::usage::record_stt("canary", 0.0);
                        (text, false)
                    }
                    Input::Pcm { samples, epoch: captured_epoch } => {
                        if muted.load(Ordering::SeqCst) || captured_epoch != epoch.load(Ordering::SeqCst) { continue; }
                        match speech::cartesia_stt(&samples).await {
                            Ok(text) if !text.trim().is_empty() => {
                                crate::usage::record_stt("cartesia", samples.len() as f64 / 16_000.0);
                                if echo_guard.matches(&text,speaker.is_some()) {
                                    if let Some(job)=&speaker {job.paused.store(false,Ordering::SeqCst);}pause_until=None;continue;
                                }
                                (text, false)
                            }
                            Ok(_) => continue,
                            Err(e) => { ui.message(format!("Ink-2 STT unavailable: {e:#}. Say that again, or set Speech → local STT.")); continue; }
                        }
                    }
                    Input::Text(text) => (text, true),
                };
                if pending_stt.is_some() && text.trim()=="/cancel" {
                    pending_stt=None;
                    ui.message("STT download cancelled.");
                    continue;
                }
                if pending_stt.is_some() && !text.starts_with('/') {
                    match stt_confirm(&text) {
                        Some(true) => {
                            match start_stt_download(&settings, &mut pending_stt, &mut stt_download) {
                                Ok(msg) => ui.message(msg),
                                Err(e) => ui.message(format!("{e:#}")),
                            }
                            continue;
                        }
                        Some(false) => {
                            pending_stt=None;
                            ui.message("STT download cancelled.");
                            continue;
                        }
                        None => {
                            ui.message("Type yes to download the local STT model, or /cancel.");
                            continue;
                        }
                    }
                }
                if typed && !text.starts_with('/') && pending_secret.is_none() && wizard.is_none() {
                    if let Some(menu)=&mut panel {
                        match menu.answer(&text,&settings,&models) {
                            Ok(crate::settings_ui::Answer::Show)=>{ui.settings(Some(menu.display(&settings,connected,&models)));continue;}
                            Ok(crate::settings_ui::Answer::Close)=>{panel=None;ui.settings(None);muted.store(user_muted || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);ui.message("Settings closed.");continue;}
                            Ok(crate::settings_ui::Answer::Command(command))=>text=command,
                            Err(e)=>{ui.message(format!("{e:#}"));continue;}
                        }
                    }
                }
                if typed && text.trim()=="/cancel" && panel.is_some() && pending_secret.is_none() && wizard.is_none() {
                    panel=None;ui.settings(None);muted.store(user_muted || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);ui.message("Settings closed.");continue;
                }
                if typed && (pending_secret.is_some() || wizard.is_some()) {
                    if text.trim()=="/quit" {break;}
                    if text.trim()=="/cancel" {
                        pending_secret=None;wizard=None;ui.secret(false);muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);ui.message("Setup cancelled.");continue;
                    }
                    if let Some(name)=pending_secret {
                        let saved = match name {
                            "typesafe" => ("TypeSafe Jev key saved in your OS credential store. Turn on Harnesses → Jev auto-select, or set Router to jev.", "TypeSafe"),
                            _ => ("Cartesia key saved in your OS credential store. Select /tts provider cartesia to use it.", "Cartesia"),
                        };
                        match config::save_secret(name,text.trim()) {Ok(())=>ui.message(saved.0),Err(e)=>ui.message(format!("Could not save {} key: {e:#}", saved.1))}
                        pending_secret=None;ui.secret(false);muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);continue;
                    }
                    if let Some(w)=&mut wizard {
                        match w.answer(&text) {
                            Ok(Some(next))=>{
                                match next.save() {
                                    Ok(())=>{settings=next;speak=settings.speak;session=Session::new(wake::WakeCode::new(&settings.wake_code,&args.wake_alias)?,settings.addressed,Duration::from_secs(settings.idle_seconds));ui.message("Setup saved. Use /tts test to hear the selected voice; /tts key adds a Cartesia key.");},
                                    Err(e)=>ui.message(format!("Could not save settings: {e:#}")),
                                }
                                wizard=None;muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                            }
                            Ok(None)=>ui.message(w.question()),
                            Err(e)=>ui.message(format!("{e:#}\n{}",w.question())),
                        }
                    }
                    continue;
                }
                if typed && text.starts_with('/') {
                    let local=match crate::dashboard::parse(&text,&settings) {Ok(command)=>command,Err(e)=>{ui.message(format!("{e:#}"));continue;}};
                    if let Some(command)=local {
                        use crate::dashboard::LocalCommand;
                        if !matches!(&command,LocalCommand::Settings|LocalCommand::SettingsNav(_)|LocalCommand::Set(..)) {panel=None;ui.settings(None);muted.store(user_muted || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);}
                        match command {
                            LocalCommand::Settings=>{
                                panel=Some(crate::settings_ui::Panel::default());
                                muted.store(true,Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                                ui.settings(Some(panel.as_ref().unwrap().display(&settings,connected,&models)));
                                ui.message("Microphone paused while settings are open. Esc until you see Activity, then say 29.");
                                if model_lookup.is_none() {
                                    let executable=args.codex_bin.clone().unwrap_or_else(||config::codex(&settings));
                                    model_lookup=Some(tokio::spawn(crate::connectors::models(executable)));
                                    ui.message("Refreshing available Codex models...");
                                }
                            }
                            LocalCommand::SettingsNav(dir)=>{
                                let Some(menu)=panel.as_mut() else {continue;};
                                let result = match dir.as_str() {
                                    "up" => { menu.nav(-1,&settings,connected); Ok(crate::settings_ui::Answer::Show) }
                                    "down" => { menu.nav(1,&settings,connected); Ok(crate::settings_ui::Answer::Show) }
                                    "enter" => menu.activate(&settings,&models,connected),
                                    _ => Ok(menu.back()),
                                };
                                match result {
                                    Ok(crate::settings_ui::Answer::Show)=>{ui.settings(Some(menu.display(&settings,connected,&models)));}
                                    Ok(crate::settings_ui::Answer::Close)=>{panel=None;ui.settings(None);muted.store(user_muted || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);ui.message("Settings closed.");}
                                    Ok(crate::settings_ui::Answer::Command(command))=>{
                                        if input_tx.try_send(Input::Text(command)).is_err() {ui.message("Input is busy; repeat the setting.");}
                                    }
                                    Err(e)=>ui.message(format!("{e:#}")),
                                }
                            }
                            LocalCommand::Help=>ui.message(crate::dashboard::help()),
                            LocalCommand::Config=>ui.message(crate::dashboard::summary(&settings)),
                            LocalCommand::Locations=>ui.message(crate::config::locations(&settings)),
                            LocalCommand::Tts=>ui.message(crate::dashboard::tts_help(&settings)),
                            LocalCommand::Setup=>{
                                if busy || speaker.is_some() {ui.message("Cancel or finish the current task/playback before setup.");continue;}
                                muted.store(true,Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                                wizard=Some(crate::dashboard::Wizard::new(&settings));ui.message(wizard.as_ref().unwrap().question());
                            }
                            LocalCommand::Secret(name)=>{
                                if !ui.interactive() {ui.message("Use acc tts setup or acc jev key in an interactive terminal to enter a hidden key.");continue;}
                                if busy || speaker.is_some() {ui.message("Cancel or finish the current task/playback before entering a key.");continue;}
                                pending_secret=Some(name);ui.secret(true);muted.store(true,Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                                ui.message(if name=="typesafe" {
                                    "Paste the TypeSafe API key (Ctrl+Shift+V / Shift+Insert), then Enter. Esc cancels. Stored in the OS credential store, not settings.json. Or: printf '%s' KEY | acc jev key"
                                } else {
                                    "Paste the Cartesia API key (Ctrl+Shift+V / Shift+Insert), then Enter. Esc cancels. Or: printf '%s' KEY | acc tts key"
                                });
                            }
                            LocalCommand::Set(key,value)=>{
                                if key=="stt.engine" {
                                    pending_stt=None;
                                    if let Some(id)=crate::stt_models::resolve(&value) {
                                        match settings.assets().and_then(|assets| crate::stt_models::confirm_text(&assets, id)) {
                                            Ok(msg) if !msg.is_empty() => {
                                                pending_stt=Some(id.into());
                                                ui.message(msg);
                                                continue;
                                            }
                                            Ok(_) => {}
                                            Err(e) => { ui.message(format!("{e:#}")); continue; }
                                        }
                                    }
                                }
                                if (key=="agent" || (key=="model" && value=="default")) && busy {
                                    pending_setting=Some((key,value,spoken_setting));pending_prompt=None;
                                    if let Some(live)=agents.get_mut(&active_harness) { live.first=None; }
                                    silence_reply=true;event_cancelled=true;speech_queue.clear();speaker=None;echo_guard.finish();
                                    if let Some(tx)=&agent_tx {let _=tx.send(agent::CommandMessage::Cancel).await;}
                                    ui.message("Interrupting current work to change agent settings...");continue;
                                }
                                let previous_agent=settings.agent.clone();
                                let mut candidate=settings.clone();
                                match candidate.set(&key,&value) {
                                    Ok(())=>{
                                        settings=candidate;
                                        cloud_stt.store(settings.stt.conversation=="cartesia", Ordering::SeqCst);
                                        lazy_stt.store(settings.stt.lazy, Ordering::SeqCst);
                                        if key=="speak" {speak=settings.speak;}
                                        if key=="chat" {ui.set_chat(&settings.chat);}
                                        if key=="wake-code" || key=="idle-seconds" || key=="addressed" {session=Session::new(wake::WakeCode::new(&settings.wake_code,&args.wake_alias)?,settings.addressed,Duration::from_secs(settings.idle_seconds));epoch.fetch_add(1,Ordering::SeqCst);}
                                        if key=="model" {
                                            active_model=settings.model.clone();
                                            if let Some(live)=agents.get(&active_harness) {let _=live.tx.try_send(agent::CommandMessage::Model(settings.model.clone()));}
                                        }
                                        if key=="agent" {
                                            active_harness=settings.agent.clone();
                                            session_pin=Some((active_harness.clone(), active_model.clone()));
                                            if let Some(live)=agents.get(&active_harness) {
                                                agent_tx=Some(live.tx.clone());
                                                connected=live.first.is_none();
                                            } else {
                                                agent_tx=None; connected=false;
                                            }
                                            if previous_agent!=settings.agent {
                                                ui.message("This conversation is pinned to that CLI until you switch again. Plugin/coding/everyday defaults stay as set.");
                                            }
                                            let missing = crate::config::harness_offers(&settings, args.codex_bin.as_ref())
                                                .into_iter()
                                                .find(|h| h.id == settings.agent && !h.found);
                                            if let Some(h) = missing {
                                                ui.message(format!("{} is not on PATH yet ({}); the next request will fail until that CLI is installed.", h.name, h.detail));
                                            }
                                        }
                                        if key=="model" && settings.model.is_none() {
                                            if let Some(live)=agents.remove(&active_harness) {
                                                let _=live.tx.try_send(agent::CommandMessage::Shutdown);
                                                live.task.abort();
                                            }
                                            agent_tx=None; connected=false;
                                            ui.message("Model reset to harness default; the next request starts a new thread.");
                                        }
                                        if key=="speak" && !speak {speech_queue.clear();speaker=None;echo_guard.finish();}
                                        muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);
                                        ui.message(format!("Saved {key}: {value}."));
                                        if key=="stt.conversation" {
                                            if settings.stt.conversation=="cartesia" {
                                                ui.message(format!("After-wake STT is now external Cartesia Ink-2. Wake spotting stays on-device ({}); cloud STT never runs while WHITE.", settings.stt.engine));
                                                if config::optional_secret("cartesia","CARTESIA_API_KEY").is_none() {
                                                    ui.message("No Cartesia key yet. /tts key saves one (same credential for TTS and Ink-2).");
                                                }
                                            } else {
                                                ui.message(format!("After-wake STT is now the local {} model, same engine as wake.", settings.stt.engine));
                                            }
                                        }
                                        if key=="stt.engine" {
                                            if let Ok(mut engine)=stt_engine.lock() { *engine = settings.stt.engine.clone(); }
                                            ui.message("Local STT engine updated. The next utterance uses it.");
                                        }
                                        if key=="routing.reasoning" {
                                            if let Some(live)=agents.remove(&active_harness) {
                                                let _=live.tx.try_send(agent::CommandMessage::Shutdown);
                                                live.task.abort();
                                            }
                                            agent_tx=None; connected=false;
                                            ui.message("Reasoning saved. The next request starts a new harness process so Antigravity --effort takes effect.");
                                        }
                                        if key=="tts.provider" && settings.tts.provider=="cartesia" {
                                            ui.message("That is spoken output (TTS), not transcription. Use Speech → After wake STT, or /stt provider cartesia, for Ink-2.");
                                        }
                                        if spoken_setting && speak {speech_queue.push_back(format!("Selected {key}: {value}."));}
                                        if let Some(menu)=&panel {ui.settings(Some(menu.display(&settings,connected,&models)));}
                                        if ["microphone","assets-dir","codex-bin"].contains(&key.as_str()) {ui.message("Restart Accessor to apply this device/path change.");}
                                    }
                                    Err(e)=>ui.message(format!("Setting not changed: {e:#}")),
                                }
                            }
                            LocalCommand::Speak(text)=>{
                                if busy || speaker.is_some() {ui.message("Cancel or finish the current task/playback before a voice test.");continue;}
                                let text=if text.is_empty(){"Hello, I'm Accessor. I'm listening, and ready to help.".into()}else{text};
                                ui.message(format!("Testing {} voice...",settings.tts.provider));speech_queue.push_back(text);
                            }
                            LocalCommand::Stt(enabled)=>{
                                panel=None;ui.settings(None);muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);
                                if busy {ui.message("Cancel or finish the agent task before changing transcription test mode.");continue;}
                                args.stt_test=enabled;session.close();epoch.fetch_add(1,Ordering::SeqCst);
                                ui.message(if enabled {"Transcription test ON: all recognized speech is displayed, with no agent. /stt off exits."}else{"Transcription test OFF. Waiting for wake code."});
                            }
                            LocalCommand::Utility(command)=>{
                                if utility.is_some() {ui.message("A diagnostic is already running; /cancel stops it.");continue;}
                                ui.message("Checking...");utility=Some(crate::dashboard::utility(command));
                            }
                            LocalCommand::Native=>{
                                if busy || speaker.is_some() {ui.message("Cancel or finish the current task/playback before opening Codex.");continue;}
                                if !ui.interactive() {ui.message("Run acc connectors setup in an interactive terminal.");continue;}
                                muted.store(true,Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                                ui.suspend()?;
                                println!("Opening Codex. Use /plugins for connections; exit Codex to return to Accessor.");
                                let result=crate::connectors::delegate(&[]).await;
                                ui.resume()?;muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                                if let Err(e)=result {ui.message(format!("Codex: {e:#}"));}
                            }
                            LocalCommand::Analytics=>ui.message(crate::usage::report()),
                            LocalCommand::Context=>ui.message(crate::route::context_report(transcript.make_contiguous(), &settings)),
                            LocalCommand::Compact=>{
                                if transcript.is_empty() {ui.message("No conversation to compact.");continue;}
                                match crate::route::compact(&crate::route::transcript_text(transcript.make_contiguous()), &settings).await {
                                    Ok(summary)=>{
                                        transcript.clear();
                                        transcript.push_back(("Summary".into(), summary.clone()));
                                        ui.message(format!("Compacted Accessor-owned history.\n{summary}"));
                                    }
                                    Err(e)=>ui.message(format!("Could not compact: {e:#}")),
                                }
                            }
                            LocalCommand::Update { check }=>{
                                if utility.is_some() {ui.message("A diagnostic is already running; /cancel stops it.");continue;}
                                if !check && busy {
                                    ui.message("Cancel the current turn first, then /update. Updating a CLI while it is working can fail.");
                                    continue;
                                }
                                if !check && !agents.is_empty() {
                                    for (_, live) in agents.drain() {
                                        let _=live.tx.try_send(agent::CommandMessage::Shutdown);
                                        live.task.abort();
                                    }
                                    agent_tx=None; connected=false;
                                    ui.message("Closed warm harness sessions so their CLIs can be replaced.");
                                }
                                ui.message(if check {
                                    "Checking harness versions..."
                                } else {
                                    "Checking harness updates and applying them if the CLI supports it..."
                                });
                                let snapshot=settings.clone();
                                utility=Some(tokio::spawn(async move { crate::updates::report(&snapshot, !check).await }));
                            }
                        }
                        continue;
                    }
                    let parts: Vec<_> = text.split_whitespace().collect();
                    match parts.as_slice() {
                        ["/quit"] => break,
                        ["/status"] => ui.message(format!("{} | {} | {} · {} · {}",if session.active(){"GREEN: conversation open"}else{"WHITE: waiting for wake code"},if busy{"agent working"}else{"idle"},crate::identity::code(&active_harness, active_model.as_deref()),active_harness,active_model.as_deref().unwrap_or("default"))),
                        ["/help"] => ui.message("/quit /stop /sleep /cancel /mute /unmute /status /approve N /deny N"),
                        ["/mute"] | ["/disconnect"] | ["/sleep"] => {
                            session.close(); epoch.fetch_add(1,Ordering::SeqCst);
                            if text.trim() == "/mute" { user_muted=true; muted.store(true,Ordering::SeqCst); ui.message("[MUTED: incoming audio discarded; device remains open]"); }
                            else {
                                think=None;silence_reply=true;speech_queue.clear();speaker=None;echo_guard.finish();
                                muted.store(user_muted,Ordering::SeqCst);
                                ui.message("Asleep. Waiting for wake code.");
                                if cues { muted.store(true,Ordering::SeqCst); if let Err(e)=audio::sleep_chime(settings.sounds.sleep) { ui.message(format!("Sleep chime unavailable: {}",safe(&e.to_string()))); } muted.store(user_muted,Ordering::SeqCst); }
                            }

                        }
                        ["/unmute"] => { if !microphone_available {ui.message("Microphone is unavailable. Check /devices and restart after setup.");continue;} user_muted=false; muted.store(panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst); epoch.fetch_add(1,Ordering::SeqCst);  }
                        ["/cancel"] | ["/stop"] => {
                            think=None;
                            if let Some(task)=utility.take() {task.abort();}
                            if text.trim()=="/stop" {
                                session.close();
                                ui.message("Stopped. Waiting for wake code.");
                                if cues { muted.store(true,Ordering::SeqCst); if let Err(e)=audio::sleep_chime(settings.sounds.sleep) { ui.message(format!("Sleep chime unavailable: {}",safe(&e.to_string()))); } muted.store(user_muted,Ordering::SeqCst); }
                            }
                            event_cancelled=true;silence_reply=true;
                            speech_queue.clear();
                            if let Some(live)=agents.get_mut(&active_harness) { live.first=None; }
                            pending_prompt=None;pending_setting=None;pending_stt=None;
                            if let Some(task)=stt_download.take() { task.abort(); }
                            if let Some(tx) = &agent_tx { let _ = tx.send(agent::CommandMessage::Cancel).await; }
                            if let Some(job) = speaker.take() { job.cancel(); muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst); }
                            epoch.fetch_add(1,Ordering::SeqCst);
                        }
                        [action @ ("/approve" | "/deny"), number] => {
                            if let (Ok(number),Some(tx)) = (number.parse(), &agent_tx) { let _ = tx.send(agent::CommandMessage::Approval { number, allow:*action=="/approve" }).await; }
                            else { ui.message("No matching approval."); }
                        }
                        _ => ui.message("Unknown command. Use /status, /cancel, /disconnect, /mute, /unmute, /approve NUMBER, /deny NUMBER, /quit."),
                    }
                    continue;
                }
                if text.trim().is_empty() {continue;}
                if user_muted && !is_event && (!typed || args.text) { continue; }
                if text.len() > 32_000 { ui.message("Input too long; discarded."); continue; }
                if args.stt_test {ui.message(format!("Transcript: {}",safe(&text)));continue;}
                if busy || speaker.is_some() {session.touch(Instant::now());}
                let was_active = session.active();
                let action = if is_event || (typed && !args.text) {Action::Prompt(text)}else{session.hear(&text,Instant::now())};
                if !is_event && !typed && !was_active && matches!(&action, Action::Open | Action::Prompt(_)) {
                    epoch.fetch_add(1,Ordering::SeqCst);
                    if !args.no_chime && !args.text { muted.store(true,Ordering::SeqCst); if let Err(e)=audio::chime(settings.sounds.wake) { ui.message(format!("Chime unavailable: {}",safe(&e.to_string()))); } muted.store(user_muted || panel.is_some() || pending_secret.is_some() || wizard.is_some(),Ordering::SeqCst); epoch.fetch_add(1,Ordering::SeqCst); }
                }
                match action {
                    Action::Ignore => continue,
                    Action::Open => {
                        if settings.addressed {
                            ui.message("Listening. Say your request. Later turns will need the wake code again.");
                        } else {
                            ui.message("Listening.");
                        }
                    }
                    Action::Disconnect => {
                        think=None;silence_reply=true;speech_queue.clear();speaker=None;echo_guard.finish();muted.store(user_muted,Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                        ui.message("Asleep. Waiting for wake code.");
                        if cues { muted.store(true,Ordering::SeqCst); if let Err(e)=audio::sleep_chime(settings.sounds.sleep) { ui.message(format!("Sleep chime unavailable: {}",safe(&e.to_string()))); } muted.store(user_muted,Ordering::SeqCst); epoch.fetch_add(1,Ordering::SeqCst); }
                    }
                    Action::Cancel | Action::Stop => {
                        think=None;
                        if matches!(action,Action::Stop) {
                            session.close();ui.message("Stopped. Waiting for wake code.");
                            if cues { muted.store(true,Ordering::SeqCst); if let Err(e)=audio::sleep_chime(settings.sounds.sleep) { ui.message(format!("Sleep chime unavailable: {}",safe(&e.to_string()))); } muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst); }
                        }
                        event_cancelled=true;silence_reply=true;speech_queue.clear();speaker=None;muted.store(user_muted || panel.is_some() || (speaker.is_some() && !settings.barge_in),Ordering::SeqCst);epoch.fetch_add(1,Ordering::SeqCst);
                        if let Some(tx)=&agent_tx { let _=tx.send(agent::CommandMessage::Cancel).await; }
                        if let Some(live)=agents.get_mut(&active_harness) { live.first=None; }
                        pending_prompt=None;pending_setting=None;pending_stt=None; }
                    Action::Prompt(text) => {
                        if pending_stt.is_some() {
                            match stt_confirm(&text) {
                                Some(true) => {
                                    match start_stt_download(&settings, &mut pending_stt, &mut stt_download) {
                                        Ok(msg) => ui.message(msg),
                                        Err(e) => ui.message(format!("{e:#}")),
                                    }
                                    continue;
                                }
                                Some(false) => {
                                    pending_stt=None;
                                    ui.message("STT download cancelled.");
                                    continue;
                                }
                                None => {
                                    ui.message("Say yes to download the local STT model, or /cancel.");
                                    continue;
                                }
                            }
                        }
                        if !is_event {
                            if (busy || speaker.is_some()) && !settings.barge_in && (!typed || args.text) {ui.message("Barge-ins are off. Use /cancel or change /settings.");continue;}
                            if let Some(command)=crate::settings_ui::voice_command(&text,&models, Some(&settings)) {
                                speech_queue.clear();speaker=None;think=None;echo_guard.finish();silence_reply=true;
                                match command {
                                    Ok((key,value))=>{if input_tx.try_send(Input::Configure{key,value,spoken:true}).is_err() {ui.message("Input is busy; please repeat the setting change.");}}
                                    Err(e)=>ui.message(format!("{e:#}")),
                                }
                                continue;
                            }
                            speech_queue.clear();speaker=None;think=None;echo_guard.finish();muted.store(user_muted || panel.is_some(),Ordering::SeqCst);
                        }
                        if busy {
                            ui.chat(Kind::User, &text);
                            silence_reply=true;event_cancelled=true;
                            if agents.get(&active_harness).is_some_and(|l| l.first.is_some()) {
                                if let Some(live)=agents.get_mut(&active_harness) { live.first=Some(text); }
                                silence_reply=false;
                            }
                            else if let Some(queued)=&mut pending_prompt {if queued.len()+text.len()<32_000 {queued.push(' ');queued.push_str(&text);}else{ui.message("Too much pending speech; wait for the follow-up to start.");}}
                            else {pending_prompt=Some(text);if let Some(tx)=&agent_tx {let _=tx.send(agent::CommandMessage::Cancel).await;}}
                            ui.message("Interrupting; your follow-up will run next.");continue;
                        }
                        if is_event {ui.message("Gmail notification received; asking the agent to handle the reply.");}
                        else {ui.chat(Kind::User, &text);}
                        silence_reply=is_event;
                        let target = if let Some((harness, model)) = &session_pin {
                            crate::route::Target {
                                harness: harness.clone(),
                                model: model.clone(),
                                kind: "pinned",
                            }
                        } else {
                            crate::route::choose(&text, &settings).await
                        };
                        if target.kind != "pinned"
                            && (target.harness != active_harness || target.model != active_model)
                        {
                            ui.message(format!("Routing {} → {} · {}", target.kind, target.harness, target.model.as_deref().unwrap_or("default")));
                        }
                        active_harness = target.harness.clone();
                        active_model = target.model.clone();
                        announce_next = settings.routing.announce
                            && crate::identity::should_announce(last_identity, Instant::now());
                        let first_for_harness = !bridged.get(&target.harness).copied().unwrap_or(false);
                        bridged.insert(target.harness.clone(), true);
                        if !is_event {
                            let blob = crate::route::transcript_text(transcript.make_contiguous());
                            if crate::usage::approx_tokens(&blob) > settings.routing.compact_tokens as usize {
                                match crate::route::compact(&blob, &settings).await {
                                    Ok(summary) => {
                                        transcript.clear();
                                        transcript.push_back(("Summary".into(), summary));
                                        ui.message(format!("Context compacted at ~{} tokens.", settings.routing.compact_tokens));
                                    }
                                    Err(e) => ui.message(format!("Auto-compact skipped: {e:#}")),
                                }
                            }
                        }
                        let outbound = if !first_for_harness
                            || !transcript.iter().any(|(role, _)| role == "Agent" || role == "Summary")
                        {
                            text.clone()
                        } else {
                            crate::route::bridge_prompt(transcript.make_contiguous(), &text, &settings).await
                        };
                        if !is_event {
                            transcript.push_back(("User".into(), text));
                            while transcript.len() > 30 { transcript.pop_front(); }
                        }
                        busy=true;
                        crate::usage::record_harness(&target.harness, crate::usage::approx_tokens(&outbound));
                        if let Some(tx) = agents.get(&target.harness).map(|live| live.tx.clone()) {
                            agent_tx = Some(tx.clone());
                            if tx.send(agent::CommandMessage::Prompt(outbound)).await.is_err() {
                                agents.remove(&target.harness);
                                busy=false; agent_tx=None; connected=false;
                                ui.message("Agent unavailable; repeat the request to reconnect.");
                            }
                        } else {
                            let (tx,task)=agent::spawn(&target.harness, agent::Options {
                                executable: config::harness_bin(&target.harness, &settings, args.codex_bin.as_ref()),
                                workspace:workspace.clone(), writable:args.workspace_write, model:target.model.clone(),
                                auto_review: settings.approvals.reviewer=="auto",
                                reasoning: settings.routing.reasoning.clone(),
                                instructions: format!(
                                    "{}\n\n{}",
                                    settings.prompt,
                                    crate::route::handoff_guide(&settings, &models)
                                ),
                            },event_tx.clone());
                            agent_tx=Some(tx.clone());
                            agents.insert(target.harness, LiveAgent { tx, task, first: Some(outbound) });
                        }
                    }
                }

            }
            Some((harness, event)) = events.recv() => {
                match event {
                    agent::Event::Ready => {
                        if harness == active_harness { connected=true; }
                        let first = agents.get_mut(&harness).and_then(|live| live.first.take());
                        let tx = agents.get(&harness).map(|live| live.tx.clone());
                        if let (Some(text), Some(tx)) = (first, tx) {
                            let _=tx.send(agent::CommandMessage::Prompt(text)).await;
                        } else if harness == active_harness { busy=false; think=None; }
                    }
                    agent::Event::Started => {
                        if harness == active_harness {
                            busy=true;
                            if !args.text && speaker.is_none() && speech_queue.is_empty() && settings.sounds.think > 0.001 {
                                match audio::think(settings.sounds.think) {
                                    Ok(cue) => think = Some(cue),
                                    Err(e) => {
                                        if !think_warned {
                                            think_warned = true;
                                            ui.message(format!("Think cue unavailable: {}", safe(&e.to_string())));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    agent::Event::Progress(text) => {
                        if harness != active_harness { continue; }
                        if pending_prompt.is_none() {ui.chat(Kind::Progress, &text);}
                        if speak && settings.speak_progress && !silence_reply {think=None;speech_queue.push_back(text);}
                    }
                    agent::Event::Reply(text) => {
                        if harness != active_harness || pending_prompt.is_some() || pending_setting.is_some() {continue;}
                        let (text, switch) = crate::route::take_handoff(&text);
                        if let Some((next, model)) = switch {
                            pending_handoff=Some((next.clone(), model));
                            ui.message(format!("Agent requested switch to {next}. That runs after this reply."));
                        }
                        if text.trim().is_empty() {
                            continue;
                        }
                        ui.chat(Kind::Agent, &text);
                        transcript.push_back(("Agent".into(), text.clone()));
                        while transcript.len() > 30 { transcript.pop_front(); }
                        if speak && !silence_reply {
                            think=None;
                            if announce_next {
                                speech_queue.push_back(crate::identity::spoken(&harness, active_model.as_deref()));
                                last_identity=Some(Instant::now());
                                announce_next=false;
                            }
                            speech_queue.push_back(text);
                        }
                    }
                    agent::Event::Tool(text) => { if harness == active_harness { ui.chat(Kind::Tool, text); } }
                    agent::Event::Done => {
                        if harness != active_harness { continue; }
                        think=None;busy=false; approval=false;session.touch(Instant::now());
                        if let (Some(event),Some(queue))=(current_event.take(),&event_queue) {queue.finish(&event,!event_cancelled)?;}
                        if let Some((key,value,spoken))=pending_setting.take() {
                            if input_tx.try_send(Input::Configure{key,value,spoken}).is_err() {ui.message("Input is busy; please repeat the setting change.");}
                        }
                        if let Some((harness, model))=pending_handoff.take() {
                            active_harness = harness.clone();
                            active_model = if model == "default" { None } else { Some(model.clone()) };
                            session_pin = Some((active_harness.clone(), active_model.clone()));
                            let existing = agents.get(&active_harness).map(|live| (live.tx.clone(), live.first.is_none()));
                            if let Some((tx, ready)) = existing {
                                agent_tx=Some(tx.clone());
                                connected=ready;
                                if active_model.is_some() {
                                    let _=tx.try_send(agent::CommandMessage::Model(active_model.clone()));
                                }
                            } else {
                                agent_tx=None; connected=false;
                            }
                            ui.message(format!(
                                "Pinned this conversation to {active_harness} · {}. Plugin/coding/everyday slots are unchanged. Say “switch to plugin” to go back.",
                                active_model.as_deref().unwrap_or("default")
                            ));
                        }
                        if let Some(text)=pending_prompt.take() {if let Some(tx)=&agent_tx {
                            silence_reply=false;busy=true;
                            if tx.send(agent::CommandMessage::Prompt(text)).await.is_err() {busy=false;connected=false;agent_tx=None;ui.message("Agent disconnected before the follow-up; please repeat it.");}
                        }}
                    }
                    agent::Event::Approval { number, detail } => { approval=true; ui.message(format!("Approval {number} (expires in 60 seconds):\n{}\nType /approve {number} or /deny {number}. Approval may allow access beyond the sandbox.",safe(&detail))); }
                    agent::Event::ApprovalClosed => approval=false,
                    agent::Event::Error(e) => {
                        agents.remove(&harness);
                        if harness == active_harness {
                            if let (Some(event),Some(queue))=(current_event.take(),&event_queue) {queue.finish(&event,false)?;}
                            busy=false;approval=false;pending_prompt=None;pending_setting=None;pending_stt=None;agent_tx=None;connected=false;think=None;session.close();epoch.fetch_add(1,Ordering::SeqCst);
                        }
                        ui.message(format!("[RED: {}]",safe(&e))); }
                    agent::Event::Note(text) => ui.message(safe(&text)),
                }

            }
            _ = tick.tick() => {
                if !busy && speaker.is_none() && speech_queue.is_empty() && current_event.is_none() && waiting_event.is_none() && wizard.is_none() && panel.is_none() && pending_secret.is_none() && !args.stt_test && last_event_poll.elapsed()>=Duration::from_secs(1) {
                    last_event_poll=Instant::now();
                    if let Some(queue)=&event_queue {
                        match queue.next() {
                            Ok(Some(event))=>{waiting_event=Some(event);},
                            Ok(None)=>{},
                            Err(e)=>{ui.message(format!("Event intake stopped: {e:#}. Fix the queue and restart with --events."));event_queue=None;}
                        }
                    }
                }
                if !busy && wizard.is_none() && panel.is_none() && pending_secret.is_none() && !args.stt_test {if let Some(event)=waiting_event.take() {
                    if let Err(error)=input_tx.try_send(Input::Trigger(event)) {
                        if let Input::Trigger(event)=error.into_inner() {waiting_event=Some(event);}
                    }
                }}
                if model_lookup.as_ref().is_some_and(|task|task.is_finished()) {
                    match model_lookup.take().unwrap().await {
                        Ok(Ok(available))=>{models=available;ui.message(format!("Loaded {} available Codex models.",models.len()));if let Some(menu)=&panel {ui.settings(Some(menu.display(&settings,connected,&models)));}}
                        Ok(Err(e))=>ui.message(format!("Model discovery unavailable: {e:#}. Cached choices remain available.")),
                        Err(e)=>ui.message(format!("Model discovery stopped: {e}")),
                    }
                }
                if utility.as_ref().is_some_and(|task|task.is_finished()) {
                    match utility.take().unwrap().await {Ok(Ok(text))=>ui.message(text),Ok(Err(e))=>ui.message(format!("Diagnostic failed: {e:#}")),Err(e)=>ui.message(format!("Diagnostic stopped: {e}"))}
                }
                if stt_download.as_ref().is_some_and(|task|task.is_finished()) {
                    match stt_download.take().unwrap().await {
                        Ok(Ok((id, msg))) => {
                            match settings.set("stt.engine", &id) {
                                Ok(()) => {
                                    if let Ok(mut engine) = stt_engine.lock() {
                                        *engine = settings.stt.engine.clone();
                                    }
                                    ui.message(msg);
                                    if let Some(menu)=&panel {ui.settings(Some(menu.display(&settings,connected,&models)));}
                                }
                                Err(e) => ui.message(format!("Downloaded, but could not save stt.engine: {e:#}")),
                            }
                        }
                        Ok(Err(e)) => ui.message(format!("STT download failed: {e:#}")),
                        Err(e) => ui.message(format!("STT download stopped: {e}")),
                    }
                }
                if let Some(text)=ui.input()? {if input_tx.try_send(Input::Text(text)).is_err() {ui.message("Input queue is busy; please enter the command again.");}}
                if busy || speaker.is_some() || !speech_queue.is_empty() {session.touch(Instant::now());}
                if session.expire(Instant::now()) {
                    think=None;epoch.fetch_add(1,Ordering::SeqCst);
                    ui.message("Asleep. Waiting for wake code.");
                    if cues { muted.store(true,Ordering::SeqCst); if let Err(e)=audio::sleep_chime(settings.sounds.sleep) { ui.message(format!("Sleep chime unavailable: {}",safe(&e.to_string()))); } muted.store(user_muted || panel.is_some() || pending_secret.is_some() || wizard.is_some(),Ordering::SeqCst); epoch.fetch_add(1,Ordering::SeqCst); }
                }
                if speaker.as_ref().is_some_and(|s|s.task.is_finished()) {
                    let mut job=speaker.take().unwrap();
                    match (&mut job.task).await {Ok(Ok(()))=>{},Ok(Err(e))=>ui.message(format!("Speech unavailable: {e:#}")),Err(e)=>ui.message(format!("Speech stopped: {e}"))}
                    echo_guard.finish();muted.store(user_muted || panel.is_some() || pending_secret.is_some() || wizard.is_some(),Ordering::SeqCst);if !settings.barge_in {epoch.fetch_add(1,Ordering::SeqCst);}session.touch(Instant::now());
                }
                if pause_until.is_some_and(|until|Instant::now()>=until) {
                    if let Some(job)=&speaker {job.paused.store(false,Ordering::SeqCst);}pause_until=None;
                }
                if speaker.is_none() {pause_until=None;if let Some(text)=speech_queue.pop_front() {
                    think=None;
                    muted.store(user_muted || panel.is_some() || pending_secret.is_some() || wizard.is_some() || !settings.barge_in,Ordering::SeqCst);if !settings.barge_in {epoch.fetch_add(1,Ordering::SeqCst);}
                    echo_guard.add(&speech::spoken_text(&text));
                    speaker=Some(speech::start(text,settings.tts.clone()));
                }}
                if eof && !busy && speaker.is_none() && speech_queue.is_empty() && utility.is_none() {break;}
            }
        }
    }
    if let (Some(event), Some(queue)) = (current_event.take(), &event_queue) {
        queue.finish(&event, false)?;
    }
    if let Some(task) = model_lookup {
        task.abort();
    }
    if let Some(task) = utility {
        task.abort();
    }
    if let Some(task) = stt_download {
        task.abort();
    }
    muted.store(true, Ordering::SeqCst);
    drop(think);
    drop(speaker);
    for (_, live) in agents.drain() {
        let _ = live.tx.send(agent::CommandMessage::Shutdown).await;
        let mut task = live.task;
        if tokio::time::timeout(Duration::from_secs(4), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }
    drop(ui);
    println!("Accessor stopped.");
    Ok(())
}

fn stt_confirm(text: &str) -> Option<bool> {
    match text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_ascii_lowercase()
        .as_str()
    {
        "yes" | "y" => Some(true),
        "no" | "n" => Some(false),
        _ => None,
    }
}

fn start_stt_download(
    settings: &config::Settings,
    pending_stt: &mut Option<String>,
    stt_download: &mut Option<tokio::task::JoinHandle<Result<(String, String)>>>,
) -> Result<String> {
    if stt_download.is_some() {
        bail!("A model download is already running.");
    }
    let id = pending_stt.take().context("No STT download pending")?;
    let assets = settings.assets()?;
    let id_task = id.clone();
    *stt_download = Some(tokio::spawn(async move {
        crate::stt_models::download(assets, &id_task)
            .await
            .map(|msg| (id_task, msg))
    }));
    Ok(format!(
        "Downloading {id}. This can take a few minutes; stay in Accessor."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn working_banner_is_not_white() {
        let text = banner(
            BannerState {
                active: false,
                busy: true,
                approval: false,
                speaking: false,
                settings_open: false,
                setup: false,
                muted: false,
            },
            "A D · antigravity · default",
        );
        assert!(!text.contains("WHITE"));
        assert!(text.contains("BLUE"));
    }
}
