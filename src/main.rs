mod agent;
mod app;
mod audio;
mod auth;
mod cli;
mod computer;
mod config;
mod connectors;
mod control;
mod dashboard;
mod desktop;
mod echo;
mod http;
mod identity;
mod journal;
mod limits;
mod markdown;
mod mcp;
mod memory;
mod noise;
mod organizer;
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
mod worker;
pub enum Input {
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
    CloudVoice {
        text: String,
        epoch: u64,
        captured_at: std::time::Instant,
    },
    IgnoredVoice {
        text: String,
        reason: String,
        epoch: u64,
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
    Error(String),
    Eof,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let result = cli::entry().await;
    usage::flush();
    result
}
