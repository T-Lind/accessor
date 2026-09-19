mod agent;
mod app;
mod audio;
mod cli;
mod config;
mod connectors;
mod dashboard;
mod echo;
mod identity;
mod markdown;
mod route;
mod session;
mod settings_ui;
mod speech;
mod stt_models;
mod triggers;
mod ui;
mod updates;
mod usage;
mod wake;
pub enum Input {
    Configure {
        key: String,
        value: String,
        spoken: bool,
    },
    Trigger(triggers::Event),
    Text(String),
    Voice {
        text: String,
        epoch: u64,
    },
    Pcm {
        samples: Vec<f32>,
        epoch: u64,
    },
    Activity {
        epoch: u64,
    },
    Error(String),
    Eof,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::entry().await
}
