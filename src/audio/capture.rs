use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;
use tracing::warn;

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

pub struct AudioCapture {
    buffer: Arc<Mutex<Vec<f32>>>,
    input_sample_rate: u32,
    channels: u16,
    stream: Option<cpal::Stream>,
    level_meter: Option<Arc<AudioLevelMeter>>,
}

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

        Ok(Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
            input_sample_rate: config.sample_rate().0,
            channels: config.channels(),
            stream: None,
            level_meter,
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
        let buffer = Arc::clone(&self.buffer);
        let level_meter = self.level_meter.clone();
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.clone().into();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        append_samples(&buffer, data);
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
                        append_samples(&buffer, &converted);
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
            std::thread::sleep(Duration::from_millis(15));
            drop(stream);
        }
        let raw = std::mem::take(&mut *self.buffer.lock().unwrap());
        let mono = downmix_to_mono(raw, self.channels);
        resample_to_16k_mono(&mono, self.input_sample_rate)
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

fn append_samples(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32]) {
    buffer.lock().unwrap().extend_from_slice(data);
}

fn downmix_to_mono(samples: Vec<f32>, channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return samples;
    }
    samples
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

fn resample_to_16k_mono(samples: &[f32], input_rate: u32) -> Result<Vec<f32>, AudioError> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    if input_rate == TARGET_SAMPLE_RATE {
        return Ok(samples.to_vec());
    }

    let ratio = input_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let output_len = ((samples.len() as f64) / ratio).round() as usize;
    let mut out = Vec::with_capacity(output_len.max(1));

    for i in 0..output_len.max(1) {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let s0 = samples.get(idx).copied().unwrap_or(0.0);
        let s1 = samples.get(idx + 1).copied().unwrap_or(s0);
        out.push(s0 + frac * (s1 - s0));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_identity_at_16k() {
        let input = vec![0.0, 0.5, -0.5, 1.0];
        let out = resample_to_16k_mono(&input, 16_000).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn resample_48k_to_16k_length() {
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.001).sin()).collect();
        let out = resample_to_16k_mono(&input, 48_000).unwrap();
        assert_eq!(out.len(), 1600);
    }
}
