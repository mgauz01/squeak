//! Peak normalization and capture diagnostics before ASR.

use std::path::Path;

use tracing::info;

pub const TARGET_PEAK: f32 = 0.7;
const QUIET_THRESHOLD: f32 = 0.05;
const HOT_THRESHOLD: f32 = 0.99;
const HOT_ATTENUATION: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioStats {
    pub samples: usize,
    pub duration_ms: u64,
    pub peak: f32,
    pub rms: f32,
    pub normalized_peak: f32,
}

pub fn analyze(samples: &[f32]) -> AudioStats {
    let peak = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);
    let rms = if samples.is_empty() {
        0.0
    } else {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    };
    let duration_ms = (samples.len() as u64 * 1000) / crate::audio::TARGET_SAMPLE_RATE as u64;

    AudioStats {
        samples: samples.len(),
        duration_ms,
        peak,
        rms,
        normalized_peak: peak,
    }
}

/// Scale quiet captures up and hot captures down toward `TARGET_PEAK`.
pub fn peak_normalize(samples: &mut [f32]) -> AudioStats {
    let mut stats = analyze(samples);
    if samples.is_empty() {
        return stats;
    }

    let peak = stats.peak;
    if peak <= 0.0 {
        return stats;
    }

    let gain = if peak < QUIET_THRESHOLD {
        TARGET_PEAK / peak
    } else if peak > HOT_THRESHOLD {
        HOT_ATTENUATION / peak
    } else {
        1.0
    };

    if (gain - 1.0).abs() > f32::EPSILON {
        for sample in samples.iter_mut() {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }

    stats.normalized_peak = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);
    stats
}

pub fn log_audio_stats(stats: AudioStats) {
    info!(
        samples = stats.samples,
        duration_ms = stats.duration_ms,
        peak = format!("{:.4}", stats.peak),
        rms = format!("{:.4}", stats.rms),
        normalized_peak = format!("{:.4}", stats.normalized_peak),
        "Audio capture stats"
    );
}

/// When `SQUEAK_DEBUG_WAV=1`, write 16 kHz mono float WAV for offline benching.
#[cfg(windows)]
pub fn maybe_write_debug_wav(samples: &[f32]) {
    if !debug_wav_enabled() {
        return;
    }

    let dir = crate::config::config_dir().join("debug");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("last.wav");
    if let Err(err) = write_wav_f32(&path, samples) {
        tracing::warn!("failed to write debug WAV: {err}");
        return;
    }
    info!("Debug WAV written to {}", path.display());
}

#[cfg(not(windows))]
pub fn maybe_write_debug_wav(_samples: &[f32]) {}

#[cfg(windows)]
fn debug_wav_enabled() -> bool {
    std::env::var("SQUEAK_DEBUG_WAV")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(windows)]
fn write_wav_f32(path: &Path, samples: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: crate::audio::TARGET_SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_unchanged() {
        let mut samples = vec![0.0; 1600];
        let stats = peak_normalize(&mut samples);
        assert_eq!(stats.peak, 0.0);
        assert!(samples.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn quiet_signal_scaled_up() {
        let mut samples = vec![0.01, -0.01, 0.008];
        let stats = peak_normalize(&mut samples);
        assert!(stats.normalized_peak >= 0.5);
        assert!(stats.normalized_peak <= 0.8);
    }

    #[test]
    fn hot_signal_attenuated() {
        let mut samples = vec![1.0, -0.99, 0.98];
        let stats = peak_normalize(&mut samples);
        assert!(stats.normalized_peak <= HOT_ATTENUATION + 0.01);
    }

    #[test]
    fn healthy_signal_unchanged() {
        let mut samples = vec![0.3, -0.25, 0.2];
        let before = samples.clone();
        peak_normalize(&mut samples);
        assert_eq!(samples, before);
    }
}
