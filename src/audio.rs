use anyhow::{bail, ensure, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::{
    borrow::Cow,
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use transcribe_rs::onnx::{
    canary::{CanaryModel, CanaryParams},
    parakeet::{ParakeetModel, ParakeetParams},
    Quantization,
};

fn speech_files_present(dir: &Path, kind: crate::stt_models::Kind) -> Result<()> {
    let names: &[&str] = match kind {
        crate::stt_models::Kind::Canary => &[
            "nemo128.onnx",
            "encoder-model.int8.onnx",
            "decoder-model.int8.onnx",
            "vocab.txt",
        ],
        crate::stt_models::Kind::Parakeet => &[
            "nemo128.onnx",
            "encoder-model.int8.onnx",
            "decoder_joint-model.int8.onnx",
            "vocab.txt",
        ],
        crate::stt_models::Kind::Whisper => &[],
    };
    for name in names {
        ensure!(
            dir.join(name).is_file(),
            "Missing {}. Accessor downloads the selected local STT model on start, or pick one in /settings.",
            dir.join(name).display()
        );
    }
    Ok(())
}

fn onnx_runtime_path() -> PathBuf {
    crate::stt_models::runtime_path(
        &crate::config::Settings::load()
            .and_then(|s| s.assets())
            .unwrap_or_else(|_| ".".into()),
    )
}

fn prepare_ort() -> Result<()> {
    static ORT: OnceLock<Result<(), String>> = OnceLock::new();
    match ORT.get_or_init(|| {
        (|| -> Result<()> {
            let runtime = onnx_runtime_path();
            ensure!(
                runtime.is_file(),
                "ONNX Runtime not found at {}. Accessor downloads it on start; set ORT_DYLIB_PATH to override.",
                runtime.display()
            );
            let environment = ort::init_from(runtime)?;
            let pool = ort::environment::GlobalThreadPoolOptions::default()
                .with_intra_threads(2)?
                .with_inter_threads(1)?
                .with_spin_control(true)?;
            environment
                .with_name("accessor")
                .with_telemetry(false)
                .with_global_thread_pool(pool)
                .commit();
            transcribe_rs::set_ort_accelerator(transcribe_rs::OrtAccelerator::CpuOnly);
            Ok(())
        })()
        .map_err(|e| format!("{e:#}"))
    }) {
        Ok(()) => Ok(()),
        Err(e) => bail!("{e}"),
    }
}

fn whisper_threads() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.get().clamp(1, 8))
        .unwrap_or(2)
}

enum Asr {
    Canary(CanaryModel),
    Parakeet(ParakeetModel),
    Whisper { cli: PathBuf, model: PathBuf },
}

pub fn load_model(dir: &Path) -> Result<CanaryModel> {
    speech_files_present(dir, crate::stt_models::Kind::Canary)?;
    prepare_ort()?;
    CanaryModel::load(dir, &Quantization::Int8).map_err(Into::into)
}

fn load_asr(assets: &Path, engine: &str) -> Result<Asr> {
    let offer = crate::stt_models::get(engine).context("Unknown local STT engine")?;
    let folder = crate::stt_models::dir(assets, offer);
    match offer.kind {
        crate::stt_models::Kind::Canary => Ok(Asr::Canary(load_model(&folder)?)),
        crate::stt_models::Kind::Parakeet => {
            speech_files_present(&folder, offer.kind)?;
            prepare_ort()?;
            Ok(Asr::Parakeet(
                ParakeetModel::load(&folder, &Quantization::Int8)
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
            ))
        }
        crate::stt_models::Kind::Whisper => {
            let cli = crate::stt_models::whisper_cli(assets)
                .context("whisper-cli is not installed yet")?;
            let model = crate::stt_models::whisper_model_file(assets, engine)?;
            ensure!(model.is_file(), "Missing {}", model.display());
            Ok(Asr::Whisper { cli, model })
        }
    }
}

const LAZY_UNLOAD: Duration = Duration::from_secs(45);

pub struct MicFlags {
    pub epoch: Arc<AtomicU64>,
    pub muted: Arc<AtomicBool>,
    pub awake: Arc<AtomicBool>,
    pub cloud_stt: Arc<AtomicBool>,
    pub lazy: Arc<AtomicBool>,
    pub engine: Arc<Mutex<String>>,
}
pub fn transcribe(model: &mut CanaryModel, samples: &[f32]) -> Result<String> {
    let samples = condition_for_asr(samples);
    Ok(model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".into()),
                max_sequence_length: 256,
                ..Default::default()
            },
        )?
        .text)
}

fn transcribe_asr(asr: &mut Asr, samples: &[f32]) -> Result<String> {
    let samples = condition_for_asr(samples);
    match asr {
        Asr::Canary(model) => transcribe(model, &samples),
        Asr::Parakeet(model) => Ok(model
            .transcribe_with(&samples, &ParakeetParams::default())
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .text),
        Asr::Whisper { cli, model } => whisper_cli_transcribe(cli, model, &samples),
    }
}

/// Bring quiet, valid microphone utterances into the range expected by local
/// models. Capture that is already loud or clipped is left alone: scaling a
/// clipped waveform cannot restore it and would hide a setup problem.
fn condition_for_asr(samples: &[f32]) -> Cow<'_, [f32]> {
    if samples.is_empty() {
        return Cow::Borrowed(samples);
    }
    let clipped = samples.iter().filter(|sample| sample.abs() >= 0.98).count();
    if clipped * 100 >= samples.len() {
        return Cow::Borrowed(samples);
    }
    let rms =
        (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt();
    if !(0.005..0.12).contains(&rms) {
        return Cow::Borrowed(samples);
    }
    let gain = (0.12 / rms).clamp(1.0, 4.0);
    if gain <= 1.01 {
        return Cow::Borrowed(samples);
    }
    Cow::Owned(
        samples
            .iter()
            .map(|sample| (sample * gain).clamp(-1.0, 1.0))
            .collect(),
    )
}

fn whisper_cli_transcribe(cli: &Path, model: &Path, samples: &[f32]) -> Result<String> {
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|s| {
            ((s.clamp(-1.0, 1.0) * 32767.0) as i16)
                .to_le_bytes()
                .into_iter()
        })
        .collect();
    let wav = wrap_pcm16_mono(&pcm, 16_000)?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("utt.wav");
    std::fs::write(&path, wav)?;
    let stem = dir.path().join("out");
    let output = std::process::Command::new(cli)
        .args(["-m"])
        .arg(model)
        .args(["-f"])
        .arg(&path)
        .args(["-t", &whisper_threads().to_string(), "-bs", "2", "-bo", "2"])
        .args(["-nt", "-np", "-nf", "-l", "en", "-otxt", "-of"])
        .arg(&stem)
        .output()
        .context("Could not start whisper-cli")?;
    ensure!(
        output.status.success(),
        "whisper-cli failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(stem.with_extension("txt")).unwrap_or_default();
    Ok(text.trim().to_owned())
}

fn ensure_asr<'a>(
    model: &'a mut Option<Asr>,
    assets: &Path,
    engine: &str,
    output: &tokio::sync::mpsc::Sender<crate::Input>,
) -> Option<&'a mut Asr> {
    if model.is_none() {
        match load_asr(assets, engine) {
            Ok(loaded) => *model = Some(loaded),
            Err(e) => {
                let _ =
                    output.try_send(crate::Input::Error(format!("Transcription failed: {e:#}")));
                return None;
            }
        }
    }
    model.as_mut()
}

#[allow(clippy::too_many_arguments)]
fn lazy_asr_tick(
    model: &mut Option<Asr>,
    assets: &Path,
    engine: &str,
    lazy: &AtomicBool,
    warm: &AtomicBool,
    awake: &AtomicBool,
    last_asr: &mut Instant,
    output: &tokio::sync::mpsc::Sender<crate::Input>,
) {
    let lazy = lazy.load(Ordering::SeqCst);
    if !lazy {
        if model.is_none() {
            let _ = ensure_asr(model, assets, engine, output);
            *last_asr = Instant::now();
        }
        return;
    }
    if model.is_none() && (warm.load(Ordering::SeqCst) || awake.load(Ordering::SeqCst)) {
        if ensure_asr(model, assets, engine, output).is_some() {
            *last_asr = Instant::now();
        }
        return;
    }
    if model.is_some()
        && !awake.load(Ordering::SeqCst)
        && !warm.load(Ordering::SeqCst)
        && last_asr.elapsed() >= LAZY_UNLOAD
    {
        *model = None;
    }
}

pub fn devices() -> Result<()> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.name().ok());
    for device in host.input_devices()? {
        let name = device.name()?;
        println!(
            "{}{}",
            name,
            if Some(&name) == default.as_ref() {
                " (default)"
            } else {
                ""
            }
        );
    }
    Ok(())
}

pub struct Capture {
    _stream: cpal::Stream,
    stop: Arc<AtomicBool>,
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
struct Chunk {
    samples: Vec<f32>,
    epoch: u64,
    at: Instant,
}
struct Utterance {
    samples: Vec<f32>,
    epoch: u64,
    at: Instant,
}

/// Fixed-memory utterance segmentation: 320ms pre-roll, 384ms trailing silence.
/// Discard overlong speech rather than executing a truncated command.
struct Segmenter {
    before: VecDeque<f32>,
    current: Vec<f32>,
    voiced: usize,
    silence: usize,
    overflow: bool,
}
impl Segmenter {
    fn new() -> Self {
        Self {
            before: VecDeque::new(),
            current: Vec::new(),
            voiced: 0,
            silence: 0,
            overflow: false,
        }
    }
    fn push(&mut self, frame: &[f32], speech: bool) -> Option<Vec<f32>> {
        if self.current.is_empty() && !self.overflow {
            self.before.extend(frame);
            while self.before.len() > 5120 {
                self.before.pop_front();
            }
            if !speech {
                return None;
            }
            self.current.extend(self.before.drain(..));
        } else if !self.overflow {
            self.current.extend_from_slice(frame);
        }
        if speech {
            self.voiced += frame.len();
            self.silence = 0;
        } else {
            self.silence += frame.len();
        }
        if self.current.len() > 16_000 * 15 {
            self.current.clear();
            self.overflow = true;
        }
        if self.silence >= 6_144 {
            let result = if !self.overflow && self.voiced >= 1536 {
                Some(std::mem::take(&mut self.current))
            } else {
                None
            };
            *self = Self::new();
            return result;
        }
        None
    }
}

pub fn listen(
    assets: &Path,
    name: Option<&str>,
    output: tokio::sync::mpsc::Sender<crate::Input>,
    flags: MicFlags,
) -> Result<Capture> {
    let engine_id = flags
        .engine
        .lock()
        .map(|e| e.clone())
        .unwrap_or_else(|_| "canary".into());
    crate::stt_models::get(&engine_id).context("Unknown local STT engine")?;
    let model = if flags.lazy.load(Ordering::SeqCst) {
        None
    } else {
        Some(load_asr(assets, &engine_id)?)
    };
    let MicFlags {
        epoch,
        muted,
        awake,
        cloud_stt,
        lazy,
        engine,
    } = flags;
    let warm = Arc::new(AtomicBool::new(false));
    let host = cpal::default_host();
    let device = if let Some(name) = name {
        host.input_devices()?
            .find(|d| d.name().ok().as_deref() == Some(name))
            .context("Requested microphone not found; run accessor devices")?
    } else {
        host.default_input_device()
            .context("No default microphone; run accessor devices")?
    };
    let supported = device.default_input_config()?;
    let config: cpal::StreamConfig = supported.clone().into();
    let (raw_tx, raw_rx) = mpsc::sync_channel::<Chunk>(256);
    // Keep only the latest completed utterance. During slow inference this lets
    // a deliberate command replace stale room noise or residual speaker echo.
    let speech_slot = Arc::new(Mutex::new(None::<Utterance>));
    let stop = Arc::new(AtomicBool::new(false));
    let lost = Arc::new(AtomicBool::new(false));
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config,
            raw_tx,
            epoch.clone(),
            muted.clone(),
            lost.clone(),
            output.clone(),
        )?,
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config,
            raw_tx,
            epoch.clone(),
            muted.clone(),
            lost.clone(),
            output.clone(),
        )?,
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config,
            raw_tx,
            epoch.clone(),
            muted.clone(),
            lost.clone(),
            output.clone(),
        )?,
        format => bail!("Unsupported microphone sample format: {format:?}"),
    };
    let rate = config.sample_rate.0 as usize;
    let dsp_stop = stop.clone();
    let dsp_epoch = epoch.clone();
    let dsp_output = output.clone();
    let dsp_muted = muted.clone();
    let dsp_warm = warm.clone();
    let dsp_speech = speech_slot.clone();
    let reference = crate::echo::connect();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut canceller = crate::echo::Canceller::new();
            let mut resampler = FftFixedIn::<f32>::new(rate, 16_000, 1024, 2, 1)?;
            let mut raw = VecDeque::new();
            let mut frames = VecDeque::new();
            let mut detector = earshot::Detector::default();
            let mut segments = Segmenter::new();
            let mut last_activity = Instant::now();
            let mut speech_run = 0_usize;
            let mut current_epoch = dsp_epoch.load(Ordering::SeqCst);
            while !dsp_stop.load(Ordering::SeqCst) {
                let chunk = match raw_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(c) => c,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let actual_epoch = dsp_epoch.load(Ordering::SeqCst);
                let discontinuity = lost.swap(false, Ordering::SeqCst);
                if chunk.epoch != actual_epoch
                    || current_epoch != actual_epoch
                    || discontinuity
                    || dsp_muted.load(Ordering::SeqCst)
                    || chunk.at.elapsed() > Duration::from_secs(1)
                {
                    raw.clear();
                    canceller.clear_capture();
                    frames.clear();
                    segments = Segmenter::new();
                    speech_run = 0;
                    dsp_warm.store(false, Ordering::SeqCst);
                    detector = earshot::Detector::default();
                    resampler.reset();
                    current_epoch = actual_epoch;
                    if discontinuity {
                        let _ = dsp_output.try_send(crate::Input::Error(
                            "Audio buffer overflow; dropped a burst and kept listening".into(),
                        ));
                        continue;
                    }
                    if chunk.epoch != actual_epoch
                        || dsp_muted.load(Ordering::SeqCst)
                        || chunk.at.elapsed() > Duration::from_secs(1)
                    {
                        continue;
                    }
                }
                raw.extend(chunk.samples);
                while raw.len() >= resampler.input_frames_next() {
                    let n = resampler.input_frames_next();
                    let data: Vec<f32> = raw.drain(..n).collect();
                    let converted = resampler.process(&[data], None)?;
                    while let Ok(chunk) = reference.try_recv() {
                        canceller.render(chunk);
                    }
                    canceller.capture(&converted[0], &mut frames)?;
                    while frames.len() >= 256 {
                        let frame: Vec<f32> = frames.drain(..256).collect();
                        let speech = detector.predict_f32(&frame) >= 0.5;
                        speech_run = if speech {
                            speech_run.saturating_add(frame.len())
                        } else {
                            0
                        };
                        dsp_warm.store(speech_run >= 2560, Ordering::SeqCst);
                        if speech_run >= 2560
                            && last_activity.elapsed() >= Duration::from_millis(200)
                        {
                            let _ = dsp_output.try_send(crate::Input::Activity {
                                epoch: current_epoch,
                            });
                            last_activity = Instant::now();
                        }
                        if let Some(samples) = segments.push(&frame, speech) {
                            if let Ok(mut slot) = dsp_speech.lock() {
                                *slot = Some(Utterance {
                                    samples,
                                    epoch: current_epoch,
                                    at: Instant::now(),
                                });
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = dsp_output.try_send(crate::Input::Error(e.to_string()));
        }
    });
    let asr_stop = stop.clone();
    let asr_awake = awake.clone();
    let asr_cloud = cloud_stt.clone();
    let asr_lazy = lazy;
    let asr_warm = warm;
    let asr_assets = assets.to_path_buf();
    let asr_engine = engine;
    std::thread::spawn(move || {
        let mut model = model;
        let mut loaded_engine = engine_id;
        let mut last_asr = Instant::now();
        while !asr_stop.load(Ordering::SeqCst) {
            let current_engine = asr_engine
                .lock()
                .map(|e| e.clone())
                .unwrap_or_else(|_| loaded_engine.clone());
            if current_engine != loaded_engine {
                model = None;
                loaded_engine = current_engine;
            }
            let u = speech_slot.lock().ok().and_then(|mut slot| slot.take());
            let Some(u) = u else {
                lazy_asr_tick(
                    &mut model,
                    &asr_assets,
                    &loaded_engine,
                    &asr_lazy,
                    &asr_warm,
                    &asr_awake,
                    &mut last_asr,
                    &output,
                );
                std::thread::sleep(Duration::from_millis(20));
                continue;
            };
            if muted.load(Ordering::SeqCst)
                || u.epoch != epoch.load(Ordering::SeqCst)
                || u.at.elapsed() > Duration::from_secs(2)
            {
                continue;
            }
            last_asr = Instant::now();
            if asr_cloud.load(Ordering::SeqCst) && asr_awake.load(Ordering::SeqCst) {
                let Some(loaded) = ensure_asr(&mut model, &asr_assets, &loaded_engine, &output)
                else {
                    break;
                };
                match transcribe_asr(loaded, &u.samples) {
                    Ok(text) if heard_word(&text) => {
                        let _ = output.try_send(crate::Input::Pcm {
                            samples: u.samples,
                            epoch: u.epoch,
                            captured_at: u.at,
                        });
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = output.try_send(crate::Input::Error(format!(
                            "Local gate for Ink-2 failed: {e}"
                        )));
                        break;
                    }
                }
                continue;
            }
            let Some(loaded) = ensure_asr(&mut model, &asr_assets, &loaded_engine, &output) else {
                break;
            };
            match transcribe_asr(loaded, &u.samples) {
                Ok(text)
                    if heard_transcript(&text)
                        && u.epoch == epoch.load(Ordering::SeqCst)
                        && !muted.load(Ordering::SeqCst) =>
                {
                    let _ = output.try_send(crate::Input::Voice {
                        text,
                        epoch: u.epoch,
                        captured_at: u.at,
                    });
                }
                Ok(_) => {}
                Err(e) => {
                    let _ =
                        output.try_send(crate::Input::Error(format!("Transcription failed: {e}")));
                    break;
                }
            }
        }
    });

    stream.play()?;
    Ok(Capture {
        _stream: stream,
        stop,
    })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: mpsc::SyncSender<Chunk>,
    epoch: Arc<AtomicU64>,
    muted: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    output: tokio::sync::mpsc::Sender<crate::Input>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let clipping_warned = Arc::new(AtomicBool::new(false));
    let level_output = output.clone();
    let mut clipping_chunks = 0_u8;
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| {
            if muted.load(Ordering::SeqCst) {
                return;
            }
            let samples: Vec<f32> = data
                .chunks_exact(channels)
                .map(|frame| {
                    frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32
                })
                .collect();
            let clipping = !samples.is_empty()
                && samples.iter().filter(|sample| sample.abs() >= 0.98).count() * 20
                    >= samples.len();
            clipping_chunks = if clipping {
                clipping_chunks.saturating_add(1)
            } else {
                0
            };
            if clipping_chunks >= 8 && !clipping_warned.swap(true, Ordering::SeqCst) {
                let _ = level_output.try_send(crate::Input::Error(
                    "Microphone level is clipping; lower the input gain, then restart. On PipeWire: wpctl set-volume @DEFAULT_AUDIO_SOURCE@ 0.40. Accessor kept listening".into(),
                ));
            }
            if tx
                .try_send(Chunk {
                    samples,
                    epoch: epoch.load(Ordering::SeqCst),
                    at: Instant::now(),
                })
                .is_err()
            {
                lost.store(true, Ordering::SeqCst);
            }
        },
        move |e| {
            let _ = output.try_send(crate::Input::Error(format!("Microphone error: {e}")));
        },
        None,
    )?)
}

#[derive(Clone, Copy)]
pub enum CueKind {
    Wake,
    Sleep,
}

pub fn chime(volume: f32) -> Result<()> {
    play_cue(CueKind::Wake, volume)
}
pub fn sleep_chime(volume: f32) -> Result<()> {
    play_cue(CueKind::Sleep, volume)
}
pub fn play_cue(kind: CueKind, volume: f32) -> Result<()> {
    let notes: &[(f32, f32, f32)] = match kind {
        CueKind::Wake => &[(0.0, 0.12, 440.0), (0.15, 0.14, 554.37)],
        CueKind::Sleep => &[
            (0.0, 0.11, 523.25),
            (0.14, 0.12, 392.0),
            (0.29, 0.16, 293.66),
        ],
    };
    let hold = match kind {
        CueKind::Wake => 350,
        CueKind::Sleep => 520,
    };
    let device = cpal::default_host()
        .default_output_device()
        .context("No speaker")?;
    let supported = device.default_output_config()?;
    let config: cpal::StreamConfig = supported.clone().into();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => notes_stream::<f32>(&device, &config, notes, None, volume)?,
        cpal::SampleFormat::I16 => notes_stream::<i16>(&device, &config, notes, None, volume)?,
        cpal::SampleFormat::U16 => notes_stream::<u16>(&device, &config, notes, None, volume)?,
        _ => bail!("Unsupported speaker format"),
    };
    stream.play()?;
    std::thread::sleep(Duration::from_millis(hold));
    Ok(())
}

pub struct Cue {
    stop: Arc<AtomicBool>,
    _stream: cpal::Stream,
}
impl Drop for Cue {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
/// Quiet looping warble while the agent is working and not speaking.
pub fn think(volume: f32) -> Result<Cue> {
    ensure!(volume > 0.001, "Think warble volume is 0");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let device = cpal::default_host()
        .default_output_device()
        .context("No speaker")?;
    let supported = device.default_output_config()?;
    let config: cpal::StreamConfig = supported.clone().into();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => warble_stream::<f32>(&device, &config, flag, volume)?,
        cpal::SampleFormat::I16 => warble_stream::<i16>(&device, &config, flag, volume)?,
        cpal::SampleFormat::U16 => warble_stream::<u16>(&device, &config, flag, volume)?,
        _ => bail!("Unsupported speaker format"),
    };
    stream.play()?;
    Ok(Cue {
        stop,
        _stream: stream,
    })
}

fn notes_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    notes: &[(f32, f32, f32)],
    stop: Option<Arc<AtomicBool>>,
    volume: f32,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut i = 0_u64;
    let rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    let notes = notes.to_vec();
    let gain = 0.08 * volume.clamp(0.0, 1.5);
    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            if stop.as_ref().is_some_and(|s| s.load(Ordering::SeqCst)) {
                for s in data.iter_mut() {
                    *s = T::from_sample(0.0);
                }
                return;
            }
            for frame in data.chunks_mut(channels) {
                let t = i as f32 / rate;
                i += 1;
                let mut sample = 0.0;
                for (start, length, freq) in &notes {
                    if t >= *start && t < start + length {
                        let local = t - start;
                        let envelope =
                            (local / 0.02).min(1.0) * ((length - local) / 0.05).clamp(0.0, 1.0);
                        sample = gain * envelope * (std::f32::consts::TAU * freq * local).sin();
                        break;
                    }
                }
                for s in frame {
                    *s = T::from_sample(sample);
                }
            }
        },
        |_| {},
        None,
    )?)
}

fn warble_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stop: Arc<AtomicBool>,
    volume: f32,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut i = 0_u64;
    let mut phase = 0.0_f32;
    let rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            if stop.load(Ordering::SeqCst) {
                for s in data.iter_mut() {
                    *s = T::from_sample(0.0);
                }
                return;
            }
            for frame in data.chunks_mut(channels) {
                let t = i as f32 / rate;
                i += 1;
                let sample = think_sample(&mut phase, t, 1.0 / rate, volume);
                for s in frame {
                    *s = T::from_sample(sample);
                }
            }
        },
        |_| {},
        None,
    )?)
}

fn think_sample(phase: &mut f32, t: f32, dt: f32, volume: f32) -> f32 {
    let lfo = (std::f32::consts::TAU * 0.18 * t).sin();
    // Keep this smooth and well below speech, but high enough for small laptop speakers.
    let freq = 330.0 + 10.0 * lfo;
    *phase += std::f32::consts::TAU * freq * dt;
    if *phase > std::f32::consts::TAU {
        *phase -= std::f32::consts::TAU;
    }
    let env = 0.62 + 0.38 * (std::f32::consts::TAU * 0.11 * t).sin();
    0.06 * volume.clamp(0.0, 1.5) * env * phase.sin()
}

pub fn heard_word(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w.chars().any(|c| c.is_ascii_alphabetic()) && w.len() >= 2)
}

fn heard_transcript(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|word| {
        word.chars().any(|c| c.is_numeric())
            || (word.chars().any(|c| c.is_alphabetic()) && word.chars().count() >= 2)
    })
}

fn system_rate(speed: f32) -> i32 {
    (((speed.clamp(0.6, 1.5) - 1.0) * 10.0).round() as i32).clamp(-10, 10)
}
fn system_wpm(speed: f32) -> u32 {
    (175.0 * speed.clamp(0.6, 1.5)).round() as u32
}

/// Speech text travels over stdin, never interpolated into a shell command.
pub async fn synthesize_system(text: &str, speed: f32) -> Result<Vec<u8>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("speech.wav");
    let mut cmd;
    if cfg!(windows) {
        cmd = tokio::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile","-NonInteractive","-Command","Add-Type -AssemblyName System.Speech; $voice = New-Object System.Speech.Synthesis.SpeechSynthesizer; $voice.Rate = [int]$env:ACC_SPEECH_RATE; $format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(22050, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono); $voice.SetOutputToWaveFile($env:ACC_SPEECH_WAV, $format); $voice.Speak([Console]::In.ReadToEnd()); $voice.Dispose()"]);
        cmd.env("ACC_SPEECH_WAV", &path);
        cmd.env("ACC_SPEECH_RATE", system_rate(speed).to_string());
    } else if cfg!(target_os = "macos") {
        cmd = tokio::process::Command::new("say");
        cmd.arg("-o")
            .arg(&path)
            .arg("--file-format=WAVE")
            .arg("--data-format=LEI16@22050")
            .arg("-r")
            .arg(system_wpm(speed).to_string());
    } else {
        cmd = tokio::process::Command::new("espeak-ng");
        cmd.arg("--stdin");
        cmd.arg("-s").arg(system_wpm(speed).to_string());
        cmd.arg("-w").arg(&path);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child.stdin.take().context("Speech stdin unavailable")?;
    let spoken: String = text.chars().take(6000).collect();
    input.write_all(spoken.as_bytes()).await?;
    drop(input);
    ensure!(
        tokio::time::timeout(Duration::from_secs(60), child.wait())
            .await??
            .success(),
        "System voice failed. Check installed voices (espeak-ng on Linux)."
    );
    Ok(std::fs::read(path)?)
}

pub fn wrap_pcm16_mono(pcm: &[u8], sample_rate: u32) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for frame in pcm.chunks_exact(2) {
            writer.write_sample(i16::from_le_bytes([frame[0], frame[1]]))?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .context("Truncated WAV header")?
            .try_into()?,
    ))
}
fn le_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .context("Truncated WAV header")?
            .try_into()?,
    ))
}

/// Decode PCM speech. Truncates a short/odd `data` chunk instead of failing the
/// way Cartesia (and some neural) WAVs do with hound.
pub fn decode_wav(bytes: &[u8]) -> Result<(u32, Vec<f32>)> {
    ensure!(
        bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "Speech audio was not a WAV file"
    );
    let mut offset = 12usize;
    let mut fmt = None;
    let mut data = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = le_u32(bytes, offset + 4)? as usize;
        let start = offset + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"fmt " {
            fmt = Some(&bytes[start..end]);
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
        }
        offset = start.saturating_add(size).saturating_add(size % 2);
        if offset <= start {
            break;
        }
    }
    let fmt = fmt.context("WAV missing fmt chunk")?;
    let data = data.context("WAV missing data chunk")?;
    ensure!(fmt.len() >= 16, "WAV fmt chunk is too small");
    let format = le_u16(fmt, 0)?;
    let channels = le_u16(fmt, 2)? as usize;
    let sample_rate = le_u32(fmt, 4)?;
    let bits = le_u16(fmt, 14)?;
    ensure!(sample_rate > 0 && channels > 0, "WAV has an invalid format");
    let frame = channels * (bits as usize / 8).max(1);
    ensure!(frame > 0, "WAV sample frame is empty");
    let usable = data.len() / frame * frame;
    let pcm = &data[..usable];
    let mut samples = Vec::new();
    match (format, bits) {
        (1, 16) => {
            for frame_bytes in pcm.chunks_exact(frame) {
                let mut sum = 0.0;
                for ch in 0..channels {
                    let o = ch * 2;
                    let s = i16::from_le_bytes([frame_bytes[o], frame_bytes[o + 1]]);
                    sum += s as f32 / 32768.0;
                }
                samples.push(sum / channels as f32);
            }
        }
        (1, 24) => {
            for frame_bytes in pcm.chunks_exact(frame) {
                let mut sum = 0.0;
                for ch in 0..channels {
                    let o = ch * 3;
                    let s = i32::from_le_bytes([
                        frame_bytes[o],
                        frame_bytes[o + 1],
                        frame_bytes[o + 2],
                        0,
                    ]) << 8
                        >> 8;
                    sum += s as f32 / 8_388_608.0;
                }
                samples.push(sum / channels as f32);
            }
        }
        (1, 32) => {
            for frame_bytes in pcm.chunks_exact(frame) {
                let mut sum = 0.0;
                for ch in 0..channels {
                    let o = ch * 4;
                    let s = i32::from_le_bytes(frame_bytes[o..o + 4].try_into()?);
                    sum += s as f32 / 2_147_483_648.0;
                }
                samples.push(sum / channels as f32);
            }
        }
        (3, 32) => {
            for frame_bytes in pcm.chunks_exact(frame) {
                let mut sum = 0.0;
                for ch in 0..channels {
                    let o = ch * 4;
                    sum += f32::from_le_bytes(frame_bytes[o..o + 4].try_into()?);
                }
                samples.push(sum / channels as f32);
            }
        }
        _ => bail!("Unsupported WAV format {format}/{bits}-bit"),
    }
    ensure!(
        !samples.is_empty(),
        "WAV contained no complete audio frames"
    );
    Ok((sample_rate, samples))
}

pub fn play_wav(bytes: &[u8], stop: &AtomicBool, paused: Arc<AtomicBool>) -> Result<()> {
    let (sample_rate, samples) = decode_wav(bytes)?;
    let device = cpal::default_host()
        .default_output_device()
        .context("No output device")?;
    let supported = device.default_output_config()?;
    let config: cpal::StreamConfig = supported.clone().into();
    let done = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let mut deadline =
        Instant::now() + Duration::from_secs_f64(samples.len() as f64 / sample_rate as f64 + 10.0);
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => wav_stream::<f32>(
            &device,
            &config,
            samples,
            sample_rate,
            done.clone(),
            failed.clone(),
            paused.clone(),
        )?,
        cpal::SampleFormat::I16 => wav_stream::<i16>(
            &device,
            &config,
            samples,
            sample_rate,
            done.clone(),
            failed.clone(),
            paused.clone(),
        )?,
        cpal::SampleFormat::U16 => wav_stream::<u16>(
            &device,
            &config,
            samples,
            sample_rate,
            done.clone(),
            failed.clone(),
            paused.clone(),
        )?,
        _ => bail!("Unsupported speaker format"),
    };
    stream.play()?;
    let mut last = Instant::now();
    while !done.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if paused.load(Ordering::SeqCst) {
            deadline += now.duration_since(last);
        }
        last = now;
        ensure!(!failed.load(Ordering::SeqCst), "Audio output failed");
        ensure!(Instant::now() < deadline, "Audio output timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Let the final scheduled output buffer drain; cancellation remains immediate.
    for _ in 0..8 {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
fn wav_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Vec<f32>,
    rate: u32,
    done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut playback = Playback {
        samples,
        position: 0.0,
        step: rate as f64 / config.sample_rate.0 as f64,
    };
    let channels = config.channels as usize;
    let reference = crate::echo::sender();
    let output_rate = config.sample_rate.0;
    Ok(device.build_output_stream(
        config,
        move |output: &mut [T], _| {
            let mut rendered = Vec::with_capacity(output.len() / channels);
            let pause = paused.load(Ordering::SeqCst);
            for frame in output.chunks_mut(channels) {
                let value = playback.next(pause).unwrap_or_else(|| {
                    done.store(true, Ordering::SeqCst);
                    0.0
                });
                for sample in frame {
                    *sample = T::from_sample(value);
                }
                rendered.push(value);
            }
            if let Some(tx) = &reference {
                let _ = tx.try_send(crate::echo::Render {
                    samples: rendered,
                    rate: output_rate,
                    at: Instant::now(),
                });
            }
        },
        move |_| {
            failed.store(true, Ordering::SeqCst);
        },
        None,
    )?)
}

struct Playback {
    samples: Vec<f32>,
    position: f64,
    step: f64,
}
impl Playback {
    fn next(&mut self, paused: bool) -> Option<f32> {
        if paused {
            return Some(0.0);
        }
        let index = self.position as usize;
        let a = self.samples.get(index)?;
        let b = self.samples.get(index + 1).unwrap_or(a);
        let sample = a + (b - a) * self.position.fract() as f32;
        self.position += self.step;
        Some(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pause_emits_silence_and_resumes_without_skipping_audio() {
        let mut p = Playback {
            samples: vec![0.2, 0.4, 0.6],
            position: 0.0,
            step: 0.5,
        };
        assert_eq!(p.next(false), Some(0.2));
        for _ in 0..16000 {
            assert_eq!(p.next(true), Some(0.0));
        }
        assert!((p.next(false).unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(p.next(false), Some(0.4));
        for _ in 0..3 {
            assert!(p.next(false).is_some());
        }
        assert!(p.next(false).is_none());
    }
    #[test]
    fn segmentation_is_bounded_and_keeps_preroll() {
        let mut s = Segmenter::new();
        let f = [0.0; 256];
        for _ in 0..100 {
            assert!(s.push(&f, false).is_none());
        }
        assert_eq!(s.before.len(), 5120);
        for _ in 0..10 {
            assert!(s.push(&f, true).is_none());
        }
        let mut result = None;
        for _ in 0..40 {
            result = s.push(&f, false).or(result);
        }
        assert!(result.unwrap().len() >= 5120 + 9 * 256 + 6144);
    }
    #[test]
    fn overlong_commands_are_discarded() {
        let mut s = Segmenter::new();
        let f = [0.0; 256];
        for _ in 0..1100 {
            assert!(s.push(&f, true).is_none());
            assert!(s.current.len() <= 240_000);
        }
        for _ in 0..50 {
            assert!(s.push(&f, false).is_none());
        }
        assert!(!s.overflow);
    }
    #[test]
    fn think_warble_stays_low_frequency() {
        let mut phase = 0.0;
        let rate = 16_000.0;
        let dt = 1.0 / rate;
        let mut prev = 0.0;
        let mut max_delta = 0.0_f32;
        let mut energy = 0.0_f32;
        for n in 0..(5 * 16_000) {
            let sample = think_sample(&mut phase, n as f32 * dt, dt, 1.0);
            max_delta = max_delta.max((sample - prev).abs());
            energy += sample * sample;
            prev = sample;
        }
        assert!(
            max_delta < 0.03,
            "think cue developed high-frequency jumps: {max_delta}"
        );
        let rms = (energy / (5.0 * rate)).sqrt();
        assert!(rms > 0.02, "think cue is too quiet: {rms}");
    }
    #[test]
    fn ink2_gate_needs_a_real_word() {
        assert!(heard_word("hello there"));
        assert!(heard_word("29 go"));
        assert!(!heard_word(""));
        assert!(!heard_word("..."));
        assert!(!heard_word("a"));
    }

    #[test]
    fn local_transcript_rejects_punctuation_only_hallucinations() {
        assert!(heard_transcript("29"));
        assert!(heard_transcript("twenty nine"));
        assert!(heard_transcript("go!"));
        assert!(!heard_transcript(""));
        assert!(!heard_transcript("-"));
        assert!(!heard_transcript("..."));
        assert!(!heard_transcript("a"));
    }

    #[test]
    fn local_asr_conditions_quiet_audio_but_not_clipped_audio() {
        let quiet = [0.0, 0.01, -0.05];
        let conditioned = condition_for_asr(&quiet);
        assert!(conditioned[2] < -0.15);
        assert!(conditioned[2] >= -0.2);

        let clipped = [0.0, 1.0, -1.0];
        assert!(matches!(condition_for_asr(&clipped), Cow::Borrowed(_)));

        let silence = [0.0; 32];
        assert!(matches!(condition_for_asr(&silence), Cow::Borrowed(_)));
    }
    fn pcm16_wav(samples: &[i16]) -> Vec<u8> {
        wrap_pcm16_mono(
            &samples
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect::<Vec<_>>(),
            16_000,
        )
        .unwrap()
    }
    #[test]
    fn decode_accepts_cartesia_odd_data_chunk() {
        let mut wav = pcm16_wav(&[1000, -1000]);
        assert!(hound::WavReader::new(std::io::Cursor::new(&wav)).is_ok());
        let size_at = wav
            .windows(8)
            .position(|w| &w[..4] == b"data")
            .expect("data")
            + 4;
        let size = u32::from_le_bytes(wav[size_at..size_at + 4].try_into().unwrap());
        wav[size_at..size_at + 4].copy_from_slice(&(size + 1).to_le_bytes());
        wav.push(0);
        assert!(hound::WavReader::new(std::io::Cursor::new(&wav)).is_err());
        let (rate, samples) = decode_wav(&wav).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(samples.len(), 2);
        assert!((samples[0] - 1000.0 / 32768.0).abs() < 1e-4);
    }
}
