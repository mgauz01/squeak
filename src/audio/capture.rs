use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use thiserror::Error;
use tracing::warn;

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
}

impl AudioCapture {
    pub fn try_new() -> Result<Self, AudioError> {
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
        })
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        self.buffer.lock().unwrap().clear();

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
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.clone().into();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| append_samples(&buffer, data),
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

        stream
            .play()
            .map_err(|_| AudioError::PermissionDenied)?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<Vec<f32>, AudioError> {
        self.stream = None;
        let raw = self.buffer.lock().unwrap().clone();
        let mono = downmix_to_mono(&raw, self.channels);
        resample_to_16k_mono(&mono, self.input_sample_rate)
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }
}

fn append_samples(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32]) {
    buffer.lock().unwrap().extend_from_slice(data);
}

fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return samples.to_vec();
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

    let ratio = TARGET_SAMPLE_RATE as f64 / input_rate as f64;
    let output_len = ((samples.len() as f64) * ratio).ceil() as usize;
    let mut resampler = FftFixedIn::<f32>::new(
        input_rate as usize,
        TARGET_SAMPLE_RATE as usize,
        1024,
        1,
        1,
    )
    .map_err(|e| AudioError::Resample(e.to_string()))?;

    let mut out = Vec::with_capacity(output_len);
    let mut pos = 0;
    while pos < samples.len() {
        let end = (pos + 1024).min(samples.len());
        let chunk = &samples[pos..end];
        let mut padded = chunk.to_vec();
        if padded.len() < 1024 {
            padded.resize(1024, 0.0);
        }
        let frames = vec![padded];
        let resampled = resampler
            .process(&frames, None)
            .map_err(|e| AudioError::Resample(e.to_string()))?;
        out.extend_from_slice(&resampled[0]);
        pos = end;
    }

    out.truncate(output_len);
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
}
