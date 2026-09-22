//! Room-noise calibration and a conservative noise gate for the STT path.
//!
//! Wake detection and segmentation are deliberately untouched: the gate is
//! applied only to completed utterances just before they are recognized, so a
//! noisy room does not distort barge-in timing or hide soft wake words.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Floor used before the room has been calibrated.
pub const DEFAULT_FLOOR_DB: f32 = -60.0;
/// Never track a floor quieter than this; digital silence is meaningless.
pub const MIN_FLOOR_DB: f32 = -100.0;
/// Never track a floor louder than this; louder input is treated as speech.
pub const MAX_FLOOR_DB: f32 = -20.0;
/// Most the gate attenuates steady noise below the floor.
pub const MAX_ATTENUATION_DB: f32 = 10.0;

const WINDOW: usize = 160; // 10 ms at 16 kHz
const OPEN_MARGIN_DB: f32 = 9.0;
const CLOSE_MARGIN_DB: f32 = 3.0;
const ATTACK: f32 = 0.02; // fast open so word onsets survive
const RELEASE: f32 = 0.002; // slow close so noise does not pump

/// RMS of a block of samples in `[-1, 1]`.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Linear RMS to dBFS (full scale is 1.0).
pub fn db(rms: f32) -> f32 {
    20.0 * rms.max(1e-10).log10()
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Robust floor estimate from observed frame levels: the 10th percentile
/// ignores a stray cough or a word spoken during calibration.
pub fn estimate_floor_db(levels: &[f32]) -> f32 {
    let mut sorted: Vec<f32> = levels.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return DEFAULT_FLOOR_DB;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() as f32 * 0.1).floor() as usize).min(sorted.len() - 1);
    sorted[index].clamp(MIN_FLOOR_DB, MAX_FLOOR_DB)
}

/// Slowly follows the quietest recent ambient level.
pub struct FloorTracker {
    floor_db: f32,
}
impl FloorTracker {
    pub fn new(floor_db: f32) -> Self {
        Self {
            floor_db: floor_db.clamp(MIN_FLOOR_DB, MAX_FLOOR_DB),
        }
    }
    pub fn floor_db(&self) -> f32 {
        self.floor_db
    }
    pub fn set(&mut self, floor_db: f32) {
        self.floor_db = floor_db.clamp(MIN_FLOOR_DB, MAX_FLOOR_DB);
    }
    /// Feed one frame the VAD considered non-speech. Tracking down is quicker
    /// than tracking up so a word cannot drag the floor upward.
    pub fn observe_silence(&mut self, frame: &[f32]) {
        let level = db(rms(frame)).clamp(MIN_FLOOR_DB, MAX_FLOOR_DB);
        let rate = if level < self.floor_db { 0.05 } else { 0.005 };
        self.floor_db += (level - self.floor_db) * rate;
        self.floor_db = self.floor_db.clamp(MIN_FLOOR_DB, MAX_FLOOR_DB);
    }
}

/// Soft-knee gate. Speech above `floor + OPEN_MARGIN_DB` is untouched; steady
/// noise at or below `floor + CLOSE_MARGIN_DB` is attenuated by at most
/// [`MAX_ATTENUATION_DB`]. The gain always starts open so the first word is not
/// clipped.
pub struct Gate {
    floor_db: f32,
}
impl Gate {
    pub fn new(floor_db: f32) -> Self {
        Self {
            floor_db: floor_db.clamp(MIN_FLOOR_DB, MAX_FLOOR_DB),
        }
    }
    fn target_gain(&self, level_db: f32) -> f32 {
        let open = self.floor_db + OPEN_MARGIN_DB;
        let close = self.floor_db + CLOSE_MARGIN_DB;
        if level_db >= open {
            1.0
        } else if level_db <= close {
            db_to_gain(-MAX_ATTENUATION_DB)
        } else {
            let t = (level_db - close) / (open - close);
            db_to_gain(-MAX_ATTENUATION_DB * (1.0 - t))
        }
    }
    /// Return a copy with steady noise attenuated. Speech is preserved.
    pub fn apply(&self, samples: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(samples.len());
        let mut gain = 1.0f32;
        let mut target = 1.0f32;
        let mut energy = 0.0f32;
        let mut count = 0usize;
        for &sample in samples {
            energy += sample * sample;
            count += 1;
            if count == WINDOW {
                target = self.target_gain(db((energy / WINDOW as f32).sqrt()));
                energy = 0.0;
                count = 0;
            }
            let coeff = if target > gain { ATTACK } else { RELEASE };
            gain += (target - gain) * coeff;
            out.push(sample * gain);
        }
        out
    }
}

/// Shared runtime state between the capture loop, the recognizer, and the UI.
pub struct Control {
    enabled: AtomicBool,
    floor_bits: AtomicU32,
    calibrate: AtomicBool,
}
impl Control {
    pub fn new(enabled: bool, floor_db: f32) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            floor_bits: AtomicU32::new(floor_db.clamp(MIN_FLOOR_DB, MAX_FLOOR_DB).to_bits()),
            calibrate: AtomicBool::new(false),
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
    pub fn set_enabled(&self, value: bool) {
        self.enabled.store(value, Ordering::Relaxed);
    }
    pub fn floor_db(&self) -> f32 {
        f32::from_bits(self.floor_bits.load(Ordering::Relaxed))
    }
    pub fn set_floor_db(&self, value: f32) {
        self.floor_bits.store(
            value.clamp(MIN_FLOOR_DB, MAX_FLOOR_DB).to_bits(),
            Ordering::Relaxed,
        );
    }
    /// A gate for one utterance, or `None` when noise suppression is off.
    pub fn gate(&self) -> Option<Gate> {
        self.enabled().then(|| Gate::new(self.floor_db()))
    }
    pub fn request_calibration(&self) {
        self.calibrate.store(true, Ordering::SeqCst);
    }
    pub fn take_calibration_request(&self) -> bool {
        self.calibrate.swap(false, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn db_and_floor_estimate_ignore_outliers() {
        assert!(db(1.0).abs() < 0.01);
        assert!((db(0.5) + 6.02).abs() < 0.05);
        let levels = vec![-80.0, -55.0, -54.0, -30.0, -10.0];
        let floor = estimate_floor_db(&levels);
        assert!((-80.0..=-55.0).contains(&floor), "{floor}");
    }
    #[test]
    fn gate_preserves_speech_and_attenuates_steady_noise() {
        let gate = Gate::new(-50.0);
        let speech: Vec<f32> = (0..3200).map(|i| 0.2 * (i as f32 * 0.3).sin()).collect();
        let out = gate.apply(&speech);
        assert!(out[3000].abs() > 0.15, "speech was gated: {}", out[3000]);
        let noise: Vec<f32> = (0..3200).map(|i| 0.004 * (i as f32 * 1.7).sin()).collect();
        let out = gate.apply(&noise);
        let before = rms(&noise);
        let after = rms(&out);
        assert!(after < before * 0.6, "noise {before} -> {after}");
    }
    #[test]
    fn tracker_follows_quiet_and_ignores_spikes() {
        let mut tracker = FloorTracker::new(-40.0);
        for _ in 0..500 {
            tracker.observe_silence(&[0.001; 256]);
        }
        assert!(tracker.floor_db() < -50.0, "{}", tracker.floor_db());
        let settled = tracker.floor_db();
        tracker.observe_silence(&[0.5; 256]);
        assert!(tracker.floor_db() < settled + 3.0, "{}", tracker.floor_db());
    }
}
