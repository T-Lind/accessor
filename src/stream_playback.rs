//! Bounded PCM playback. The callback never waits for the network; starvation or
//! active user capture emits silence without consuming speech. Echo references
//! contain exactly the samples submitted to the speaker, including that silence.
use anyhow::{bail, ensure, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

pub struct Player {
    task: tokio::task::JoinHandle<Result<()>>,
    stop: Arc<AtomicBool>,
}
impl Player {
    pub fn start(
        receiver: mpsc::Receiver<Vec<f32>>,
        capture: Arc<crate::audio::SpeechState>,
        playing: Arc<AtomicBool>,
        volume: f32,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        Self {
            stop,
            task: tokio::task::spawn_blocking(move || {
                play(receiver, cancel, capture, playing, volume)
            }),
        }
    }
    pub async fn finish(mut self) -> Result<()> {
        (&mut self.task).await?
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.task.abort();
    }
}

struct Buffer {
    receiver: mpsc::Receiver<Vec<f32>>,
    samples: VecDeque<f32>,
    fraction: f64,
    step: f64,
    ended: bool,
    ready: bool,
}
impl Buffer {
    fn new(receiver: mpsc::Receiver<Vec<f32>>, rate: u32) -> Self {
        Self {
            receiver,
            samples: VecDeque::new(),
            fraction: 0.0,
            step: 16000.0 / rate as f64,
            ended: false,
            ready: false,
        }
    }
    fn next(&mut self, paused: bool) -> Option<f32> {
        if paused {
            return Some(0.0);
        }
        // 120ms prebuffer; every producer packet is at most 100ms.
        while !self.ended && self.samples.len() < 1920 {
            match self.receiver.try_recv() {
                Ok(samples) => self.samples.extend(samples),
                Err(mpsc::error::TryRecvError::Disconnected) => self.ended = true,
                Err(mpsc::error::TryRecvError::Empty) => break,
            }
        }
        if self.ended && self.samples.is_empty() {
            return None;
        }
        if !self.ready {
            self.ready = self.samples.len() >= 1920 || self.ended;
        }
        if !self.ready {
            return Some(0.0);
        }
        let needed = (self.fraction + self.step).floor() as usize;
        if !self.ended && self.samples.len() < needed.max(2) {
            self.ready = false;
            return Some(0.0);
        }
        let a = *self.samples.front()?;
        let b = self.samples.get(1).copied().unwrap_or(a);
        let value = a + (b - a) * self.fraction as f32;
        self.fraction += self.step;
        let consumed = self.fraction.floor() as usize;
        for _ in 0..consumed {
            self.samples.pop_front();
        }
        self.fraction -= consumed as f64;
        Some(value)
    }
}

struct State {
    stop: Arc<AtomicBool>,
    done: AtomicBool,
    failed: AtomicBool,
    capture: Arc<crate::audio::SpeechState>,
    playing: Arc<AtomicBool>,
}
struct Reset(Arc<State>);
impl Drop for Reset {
    fn drop(&mut self) {
        self.0.capture.playback(false);
        self.0.playing.store(false, Ordering::SeqCst);
    }
}
fn play(
    receiver: mpsc::Receiver<Vec<f32>>,
    stop: Arc<AtomicBool>,
    capture: Arc<crate::audio::SpeechState>,
    playing: Arc<AtomicBool>,
    volume: f32,
) -> Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .context("No output device")?;
    let supported = device.default_output_config()?;
    let config: cpal::StreamConfig = supported.clone().into();
    let state = Arc::new(State {
        stop,
        done: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        capture,
        playing,
    });
    let _reset = Reset(state.clone());
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            stream::<f32>(&device, &config, receiver, state.clone(), volume)?
        }
        cpal::SampleFormat::I16 => {
            stream::<i16>(&device, &config, receiver, state.clone(), volume)?
        }
        cpal::SampleFormat::U16 => {
            stream::<u16>(&device, &config, receiver, state.clone(), volume)?
        }
        _ => bail!("Unsupported speaker format"),
    };
    stream.play()?;
    let began = Instant::now();
    while !state.stop.load(Ordering::SeqCst) && !state.done.load(Ordering::SeqCst) {
        ensure!(
            !state.failed.load(Ordering::SeqCst),
            "Streaming audio output failed"
        );
        ensure!(
            began.elapsed() < Duration::from_secs(90),
            "Streaming audio output timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..8 {
        if state.stop.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
fn stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    receiver: mpsc::Receiver<Vec<f32>>,
    state: Arc<State>,
    volume: f32,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut buffer = Buffer::new(receiver, config.sample_rate.0);
    let channels = config.channels as usize;
    let rate = config.sample_rate.0;
    let reference = crate::echo::sender();
    let failed = state.clone();
    Ok(device.build_output_stream(
        config,
        move |output: &mut [T], _| {
            let mut rendered = Vec::with_capacity(output.len() / channels);
            let paused = state.capture.holding() || state.stop.load(Ordering::SeqCst);
            for frame in output.chunks_mut(channels) {
                let value = buffer.next(paused).unwrap_or_else(|| {
                    state.done.store(true, Ordering::SeqCst);
                    0.0
                });
                if buffer.ready && !paused {
                    state.capture.playback(true);
                    state.playing.store(true, Ordering::SeqCst);
                }
                let value = (value * volume.clamp(0.0, 1.5)).clamp(-1.0, 1.0);
                for sample in frame {
                    *sample = T::from_sample(value);
                }
                rendered.push(value);
            }
            if let Some(tx) = &reference {
                let _ = tx.try_send(crate::echo::Render {
                    samples: rendered,
                    rate,
                    at: Instant::now(),
                });
            }
        },
        move |_| {
            failed.failed.store(true, Ordering::SeqCst);
        },
        None,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_handles_underrun_pause_resampling_and_final_tail() {
        let (tx, rx) = mpsc::channel(2);
        let mut b = Buffer::new(rx, 32000);
        assert_eq!(b.next(false), Some(0.0));
        tx.try_send(vec![0.2, 0.4, 0.6]).unwrap();
        assert_eq!(b.next(false), Some(0.0)); // still prebuffering
        drop(tx);
        assert_eq!(b.next(true), Some(0.0));
        let out: Vec<f32> = std::iter::from_fn(|| b.next(false)).collect();
        assert_eq!(out.len(), 6);
        assert!((out[1] - 0.3).abs() < 0.0001);
        assert_eq!(out.last(), Some(&0.6));
    }
    #[test]
    fn underflow_does_not_skip_unreceived_samples() {
        let (tx, rx) = mpsc::channel(2);
        tx.try_send(vec![0.2; 1920]).unwrap();
        let mut b = Buffer::new(rx, 16000);
        for _ in 0..1919 {
            assert_eq!(b.next(false), Some(0.2));
        }
        assert_eq!(b.next(false), Some(0.0));
        tx.try_send(vec![0.4; 1920]).unwrap();
        assert_eq!(b.next(false), Some(0.2));
        assert_eq!(b.next(false), Some(0.4));
    }
}
