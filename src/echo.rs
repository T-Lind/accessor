//! Bounded speaker-reference handoff and WebRTC AEC3 at 16 kHz.
use anyhow::Result;
use std::{
    collections::VecDeque,
    sync::{mpsc, Mutex, OnceLock},
    time::{Duration, Instant},
};
pub struct Render {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub at: Instant,
}
static REFERENCE: OnceLock<Mutex<Option<mpsc::SyncSender<Render>>>> = OnceLock::new();
pub fn connect() -> mpsc::Receiver<Render> {
    let (tx, rx) = mpsc::sync_channel(32);
    *REFERENCE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(tx);
    rx
}
pub fn sender() -> Option<mpsc::SyncSender<Render>> {
    REFERENCE.get()?.lock().ok()?.clone()
}
pub struct Canceller {
    apm: sonora::AudioProcessing,
    render: VecDeque<f32>,
    capture: VecDeque<f32>,
    position: f64,
    previous: Option<f32>,
}
impl Canceller {
    pub fn new() -> Self {
        let config = sonora::Config {
            echo_canceller: Some(sonora::config::EchoCanceller::default()),
            ..Default::default()
        };
        Self {
            apm: sonora::AudioProcessing::builder()
                .config(config)
                .capture_config(sonora::StreamConfig::new(16000, 1))
                .render_config(sonora::StreamConfig::new(16000, 1))
                .build(),
            render: VecDeque::new(),
            capture: VecDeque::new(),
            position: 0.,
            previous: None,
        }
    }
    pub fn render(&mut self, chunk: Render) {
        if chunk.at.elapsed() > Duration::from_millis(500) || chunk.rate == 0 {
            return;
        }
        let mut data = Vec::with_capacity(chunk.samples.len() + 1);
        if let Some(p) = self.previous {
            data.push(p);
        }
        data.extend(chunk.samples);
        let step = chunk.rate as f64 / 16000.;
        while self.position + 1. < data.len() as f64 {
            let i = self.position as usize;
            let f = self.position.fract() as f32;
            self.render.push_back(data[i] + (data[i + 1] - data[i]) * f);
            self.position += step;
        }
        if let Some(last) = data.last() {
            self.previous = Some(*last);
            self.position -= data.len().saturating_sub(1) as f64;
        }
        while self.render.len() > 8000 {
            self.render.pop_front();
        }
    }
    pub fn capture(&mut self, input: &[f32], out: &mut VecDeque<f32>) -> Result<()> {
        self.capture.extend(input);
        while self.capture.len() >= 160 {
            // Render frames come from actual output callbacks, not synthesis time.
            // AEC3 estimates acoustic delay; the hint covers typical device buffering.
            while self.render.len() >= 160 {
                let r: Vec<_> = self.render.drain(..160).collect();
                let mut dest = [0.; 160];
                self.apm.process_render_f32(&[&r], &mut [&mut dest])?;
            }
            let c: Vec<_> = self.capture.drain(..160).collect();
            let mut dest = [0.; 160];
            self.apm.set_stream_delay_ms(60)?;
            self.apm.process_capture_f32(&[&c], &mut [&mut dest])?;
            out.extend(dest);
        }
        Ok(())
    }
    pub fn clear_capture(&mut self) {
        self.capture.clear();
    }
}

/// Suppress recognizable residual self-speech after AEC, including short tails.
pub struct TextGuard {
    recent: VecDeque<(String, Instant)>,
}
fn words(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
impl TextGuard {
    pub fn new() -> Self {
        Self {
            recent: VecDeque::new(),
        }
    }
    pub fn add(&mut self, text: &str) {
        self.recent.push_back((words(text), Instant::now()));
        while self.recent.len() > 4 {
            self.recent.pop_front();
        }
    }
    pub fn finish(&mut self) {
        if let Some((_, at)) = self.recent.back_mut() {
            *at = Instant::now();
        }
    }
    pub fn matches(&self, text: &str, playing: bool) -> bool {
        let text = words(text);
        if text.len() < 8 {
            return false;
        }
        self.recent.iter().any(|(spoken, at)| {
            (playing || at.elapsed() < Duration::from_secs(2))
                && (spoken.contains(&text) || {
                    let tokens: Vec<_> = text.split_whitespace().collect();
                    tokens.len() >= 4
                        && tokens
                            .iter()
                            .filter(|word| spoken.split_whitespace().any(|s| s == **word))
                            .count() as f32
                            / tokens.len() as f32
                            >= 0.9
                })
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn residual_echo_and_new_request() {
        let mut guard = TextGuard::new();
        guard.add("I'll look for an AI model named Jev and check similar names.");
        assert!(guard.matches("an AI model named Jev", true));
        assert!(!guard.matches("29 stop", true));
        assert!(!guard.matches("Actually, search for Gemma instead", true));
    }
    #[test]
    fn synthetic_delayed_echo_is_reduced() {
        let mut aec = Canceller::new();
        let mut seed = 42_u32;
        let mut history = VecDeque::from(vec![0_f32; 960]);
        let mut before = 0_f64;
        let mut after = 0_f64;
        for frame in 0..800 {
            let render: Vec<_> = (0..160)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    ((seed >> 16) as f32 / 65536. - 0.5) * 0.4
                })
                .collect();
            history.extend(render.iter().copied());
            let mic: Vec<_> = history.drain(..160).map(|v| v * 0.6).collect();
            aec.render(Render {
                samples: render,
                rate: 16000,
                at: Instant::now(),
            });
            let mut output = VecDeque::new();
            aec.capture(&mic, &mut output).unwrap();
            if frame > 600 {
                before += mic.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
                after += output.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
            }
        }
        assert!(
            after < before * 0.5,
            "echo energy {after} versus input {before}"
        );
    }
}
