mod agent;
mod app;
mod audio;
mod auth;
mod cli;
mod computer;
mod config;
mod connectors;
mod control;
mod conversation;
mod dashboard;
mod decider;
mod desktop;
mod echo;
mod http;
mod human;
mod identity;
mod jobs;
mod journal;
mod limits;
mod markdown;
mod mascot;
mod masterlog;
mod mcp;
mod memory;
mod music;
mod noise;
mod notifications;
mod organizer;
mod presence;
mod process_tree;
mod quota;
mod route;
mod session;
mod settings_api;
mod settings_ui;
mod speech;
mod stream_playback;
mod stt_models;
mod stt_stream;
mod triggers;
mod ui;
mod updates;
mod usage;
mod wake;
mod wake_kws;
mod worker;
#[allow(clippy::large_enum_variant)]
pub enum Input {
    Human(human::Request),
    StartJob {
        job: Box<jobs::Job>,
        prompt: String,
    },
    Control(control::Request),
    Configure {
        key: String,
        value: String,
        spoken: bool,
    },
    Trigger(triggers::Event),
    Scheduled(organizer::Task),
    Internal(String),
    Text(String),
    WakeProbe {
        text: String,
        error: Option<String>,
        epoch: u64,
        decode_ms: u64,
    },
    /// A local keyword-spotter hit for the wake phrase. It opens listening or
    /// interrupts playback; local Canary recognition still finalizes the request.
    Kws {
        keyword: String,
        epoch: u64,
    },
    CloudVoice {
        text: String,
        epoch: u64,
        captured_at: std::time::Instant,
    },
    IgnoredVoice {
        text: String,
        reason: String,
        epoch: u64,
        confidence: Option<audio::Confidence>,
    },
    NoiseCalibrated {
        floor_db: f32,
        epoch: u64,
    },
    GatedVoice {
        decision: route::InputDecision,
        text: String,
        epoch: u64,
        captured_at: std::time::Instant,
    },
    Voice {
        text: String,
        epoch: u64,
        captured_at: std::time::Instant,
        /// Canary decoding confidence when the local engine reports it.
        confidence: Option<audio::Confidence>,
    },
    Pcm {
        streamed: Option<stt_stream::Transcript>,
        samples: Vec<f32>,
        local_text: String,
        epoch: u64,
        captured_at: std::time::Instant,
    },
    Activity {
        epoch: u64,
    },
    /// A tone chosen off the hot path, tied to the current reply generation.
    Mood {
        mood: mascot::Mood,
        generation: u64,
    },
    /// Recoverable audio or background failure; never ends the session.
    Warning(String),
    Error(String),
    Eof,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let result = cli::entry().await;
    jobs::flush().await;
    usage::flush();
    masterlog::flush();
    result
}
