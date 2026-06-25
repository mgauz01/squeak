#[cfg(windows)]
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;
#[cfg(windows)]
use tracing::warn;

#[cfg(windows)]
use crate::audio::level_meter::AudioLevelMeter;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no input device available")]
    NoInputDevice,

    #[error("failed to query device config: {0}")]
    Config(String),

    #[error("failed to build input stream: {0}")]
    Stream(String),

    #[error("failed to resample audio: {0}")]
    Resample(String),

    #[error("microphone permission denied or device unavailable")]
    PermissionDenied,
}

#[cfg(windows)]
pub struct AudioCapture {
    buffer: Arc<Mutex<Vec<f32>>>,
    #[allow(dead_code)]
    input_sample_rate: u32,
    #[allow(dead_code)]
    channels: u16,
    stream: Option<cpal::Stream>,
    level_meter: Option<Arc<AudioLevelMeter>>,
    resampler: Arc<Mutex<OnTheFlyResampler>>,
}

#[cfg(windows)]
impl AudioCapture {
    pub fn try_new() -> Result<Self, AudioError> {
        Self::try_new_with_meter(None)
    }

    pub fn try_new_with_meter(
        level_meter: Option<Arc<AudioLevelMeter>>,
    ) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(AudioError::NoInputDevice)?;

        let config = device
            .default_input_config()
            .map_err(|e| AudioError::Config(e.to_string()))?;

        let input_sample_rate = config.sample_rate().0;
        let channels = config.channels();

        Ok(Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
            input_sample_rate,
            channels,
            stream: None,
            level_meter,
            resampler: Arc::new(Mutex::new(OnTheFlyResampler::new(
                input_sample_rate,
                channels,
            ))),
        })
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        if self.stream.is_some() {
            return Ok(());
        }

        self.buffer.lock().unwrap().clear();
        if let Some(meter) = &self.level_meter {
            meter.reset();
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(AudioError::NoInputDevice)?;

        let config = device
            .default_input_config()
            .map_err(|e| AudioError::Config(e.to_string()))?;

        self.input_sample_rate = config.sample_rate().0;
        self.channels = config.channels();
        {
            let mut resampler = self.resampler.lock().unwrap();
            resampler.reset(self.input_sample_rate, self.channels);
        }

        let buffer = Arc::clone(&self.buffer);
        let resampler = Arc::clone(&self.resampler);
        let level_meter = self.level_meter.clone();
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.clone().into();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        process_and_append(&buffer, &resampler, data);
                        if let Some(meter) = &level_meter {
                            meter.update_from_chunk(data);
                        }
                    },
                    move |err| warn!("audio stream error: {err}"),
                    None,
                )
                .map_err(|e| AudioError::Stream(e.to_string()))?,
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        let converted: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        process_and_append(&buffer, &resampler, &converted);
                        if let Some(meter) = &level_meter {
                            meter.update_from_chunk(&converted);
                        }
                    },
                    move |err| warn!("audio stream error: {err}"),
                    None,
                )
                .map_err(|e| AudioError::Stream(e.to_string()))?,
            other => {
                return Err(AudioError::Stream(format!(
                    "unsupported sample format: {other:?}"
                )));
            }
        };

        stream.play().map_err(|_| AudioError::PermissionDenied)?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<Vec<f32>, AudioError> {
        if let Some(stream) = self.stream.take() {
            let _ = stream.pause();
            // Let the cpal callback drain its final partial buffer into our mutex.
            // Short sleep is sufficient; the callback runs on the audio thread and typically
            // finishes within 1-2 ms after pause().
            std::thread::sleep(Duration::from_millis(2));
            drop(stream);
        }
        let samples = std::mem::take(&mut *self.buffer.lock().unwrap());
        Ok(samples)
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }

    /// Stop capture and discard buffered audio (e.g. after a short tap that never started PTT).
    pub fn disarm(&mut self) {
        self.stream = None;
        self.buffer.lock().unwrap().clear();
        if let Some(meter) = &self.level_meter {
            meter.reset();
        }
    }
}

#[cfg(windows)]
fn process_and_append(
    buffer: &Arc<Mutex<Vec<f32>>>,
    resampler: &Arc<Mutex<OnTheFlyResampler>>,
    data: &[f32],
) {
    let mut resampler = resampler.lock().unwrap();
    let processed = resampler.process(data);
    if !processed.is_empty() {
        buffer.lock().unwrap().extend_from_slice(&processed);
    }
}

#[allow(dead_code)]
struct OnTheFlyResampler {
    input_rate: u32,
    channels: u16,
    /// Carried-over position in the input stream (in terms of output samples).
    /// Used to maintain phase across chunks.
    src_pos_accum: f64,
    /// The very last sample from the previous chunk, for linear interpolation.
    last_sample: f32,
    /// Partial mono samples from the end of a chunk if it didn't align with channels.
    leftover_samples: Vec<f32>,
}

#[allow(dead_code)]
impl OnTheFlyResampler {
    fn new(input_rate: u32, channels: u16) -> Self {
        Self {
            input_rate,
            channels: channels.max(1),
            src_pos_accum: 0.0,
            last_sample: 0.0,
            leftover_samples: Vec::new(),
        }
    }

    fn reset(&mut self, input_rate: u32, channels: u16) {
        self.input_rate = input_rate;
        self.channels = channels.max(1);
        self.src_pos_accum = 0.0;
        self.last_sample = 0.0;
        self.leftover_samples.clear();
    }

    fn process(&mut self, data: &[f32]) -> Vec<f32> {
        if data.is_empty() {
            return Vec::new();
        }

        // 1. Downmix to mono (including any leftovers from last time)
        let mut mono =
            Vec::with_capacity((data.len() + self.leftover_samples.len()) / self.channels as usize);
        let mut idx = 0;

        // Handle leftovers
        if !self.leftover_samples.is_empty() {
            let needed = self.channels as usize - self.leftover_samples.len();
            if data.len() >= needed {
                let mut frame = std::mem::take(&mut self.leftover_samples);
                frame.extend_from_slice(&data[..needed]);
                mono.push(frame.iter().sum::<f32>() / self.channels as f32);
                idx = needed;
            } else {
                self.leftover_samples.extend_from_slice(data);
                return Vec::new();
            }
        }

        // Process remaining full frames
        let ch = self.channels as usize;
        while idx + ch <= data.len() {
            let frame = &data[idx..idx + ch];
            mono.push(frame.iter().sum::<f32>() / ch as f32);
            idx += ch;
        }

        // Save leftovers for next time
        if idx < data.len() {
            self.leftover_samples.extend_from_slice(&data[idx..]);
        }

        if mono.is_empty() {
            return Vec::new();
        }

        // 2. Resample to 16kHz
        if self.input_rate == TARGET_SAMPLE_RATE {
            self.last_sample = mono[mono.len() - 1];
            return mono;
        }

        let ratio = self.input_rate as f64 / TARGET_SAMPLE_RATE as f64;

        // Estimate output length based on accumulated position
        let start_pos = self.src_pos_accum;
        let end_pos = start_pos + (mono.len() as f64 / ratio);

        let start_idx = start_pos.ceil() as usize;
        let end_idx = end_pos.ceil() as usize;
        let output_len = end_idx.saturating_sub(start_idx);

        let mut out = Vec::with_capacity(output_len);

        for i in 0..output_len {
            let current_output_idx = (start_idx + i) as f64;
            let src_pos = current_output_idx * ratio - (start_pos * ratio);

            let idx = src_pos.floor() as isize;
            let frac = (src_pos - idx as f64) as f32;

            let s0 = if idx < 0 {
                self.last_sample
            } else {
                mono[idx as usize]
            };

            let s1 = if (idx + 1) as usize >= mono.len() {
                mono[mono.len() - 1]
            } else {
                mono[(idx + 1) as usize]
            };

            out.push(s0 + frac * (s1 - s0));
        }

        self.src_pos_accum = end_pos % 1.0;
        self.last_sample = mono[mono.len() - 1];

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_identity_at_16k() {
        let mut resampler = OnTheFlyResampler::new(16_000, 1);
        let input = vec![0.0, 0.5, -0.5, 1.0];
        let out = resampler.process(&input);
        assert_eq!(out, input);
    }

    #[test]
    fn resample_48k_to_16k_length() {
        let mut resampler = OnTheFlyResampler::new(48_000, 1);
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.001).sin()).collect();
        let out = resampler.process(&input);
        assert_eq!(out.len(), 1600);
    }

    #[test]
    fn resample_chunked_48k_to_16k() {
        let mut resampler = OnTheFlyResampler::new(48_000, 1);
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.001).sin()).collect();

        let mut chunked_out = Vec::new();
        for chunk in input.chunks(480) {
            chunked_out.extend(resampler.process(chunk));
        }

        assert_eq!(chunked_out.len(), 1600);

        // Compare with non-chunked
        let mut resampler2 = OnTheFlyResampler::new(48_000, 1);
        let direct_out = resampler2.process(&input);

        for (i, (&a, &b)) in chunked_out.iter().zip(direct_out.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "Mismatch at index {}: {} != {}",
                i,
                a,
                b
            );
        }
    }
}
