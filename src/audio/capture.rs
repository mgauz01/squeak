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
struct CaptureState {
    buffer: Vec<f32>,
    resampler: OnTheFlyResampler,
    /// Reused i16→f32 conversion scratch (i16 devices only).
    i16_scratch: Vec<f32>,
}

#[cfg(windows)]
pub struct AudioCapture {
    state: Arc<Mutex<CaptureState>>,
    #[allow(dead_code)]
    input_sample_rate: u32,
    #[allow(dead_code)]
    channels: u16,
    stream: Option<cpal::Stream>,
    level_meter: Option<Arc<AudioLevelMeter>>,
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
            state: Arc::new(Mutex::new(CaptureState {
                buffer: Vec::new(),
                resampler: OnTheFlyResampler::new(input_sample_rate, channels),
                i16_scratch: Vec::new(),
            })),
            input_sample_rate,
            channels,
            stream: None,
            level_meter,
        })
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        if self.stream.is_some() {
            return Ok(());
        }

        {
            let mut state = self.state.lock().unwrap();
            state.buffer.clear();
        }
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
            let mut state = self.state.lock().unwrap();
            state.resampler.reset(self.input_sample_rate, self.channels);
        }

        let state = Arc::clone(&self.state);
        let level_meter = self.level_meter.clone();
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.clone().into();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        let mut st = state.lock().unwrap();
                        let start = st.buffer.len();
                        let CaptureState {
                            buffer, resampler, ..
                        } = &mut *st;
                        resampler.process_into(data, buffer);
                        if let Some(meter) = &level_meter {
                            if buffer.len() > start {
                                meter.update_from_chunk(&buffer[start..]);
                            }
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
                        let mut st = state.lock().unwrap();
                        let CaptureState {
                            buffer,
                            resampler,
                            i16_scratch,
                        } = &mut *st;
                        i16_scratch.clear();
                        i16_scratch.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                        let start = buffer.len();
                        resampler.process_into(i16_scratch.as_slice(), buffer);
                        if let Some(meter) = &level_meter {
                            if buffer.len() > start {
                                meter.update_from_chunk(&buffer[start..]);
                            }
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
        let samples = std::mem::take(&mut self.state.lock().unwrap().buffer);
        Ok(samples)
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }

    /// Stop capture and discard buffered audio (e.g. after a short tap that never started PTT).
    pub fn disarm(&mut self) {
        self.stream = None;
        self.state.lock().unwrap().buffer.clear();
        if let Some(meter) = &self.level_meter {
            meter.reset();
        }
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
    /// Reused downmix scratch — avoids a per-callback allocation.
    mono: Vec<f32>,
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
            mono: Vec::new(),
        }
    }

    fn reset(&mut self, input_rate: u32, channels: u16) {
        self.input_rate = input_rate;
        self.channels = channels.max(1);
        self.src_pos_accum = 0.0;
        self.last_sample = 0.0;
        self.leftover_samples.clear();
        self.mono.clear();
    }

    /// Downmix to mono + resample to 16 kHz, appending results into `out`.
    fn process_into(&mut self, data: &[f32], out: &mut Vec<f32>) {
        if data.is_empty() {
            return;
        }
        let ch = self.channels as usize;

        // 1. Downmix to mono (reusing the scratch buffer) including any leftovers.
        self.mono.clear();
        let mut idx = 0;
        if !self.leftover_samples.is_empty() {
            let needed = ch - self.leftover_samples.len();
            if data.len() >= needed {
                let sum: f32 =
                    self.leftover_samples.iter().sum::<f32>() + data[..needed].iter().sum::<f32>();
                self.mono.push(sum / ch as f32);
                self.leftover_samples.clear();
                idx = needed;
            } else {
                self.leftover_samples.extend_from_slice(data);
                return;
            }
        }
        while idx + ch <= data.len() {
            let frame = &data[idx..idx + ch];
            self.mono.push(frame.iter().sum::<f32>() / ch as f32);
            idx += ch;
        }
        if idx < data.len() {
            self.leftover_samples.extend_from_slice(&data[idx..]);
        }
        if self.mono.is_empty() {
            return;
        }

        // 2. Resample to 16 kHz.
        if self.input_rate == TARGET_SAMPLE_RATE {
            self.last_sample = self.mono[self.mono.len() - 1];
            out.extend_from_slice(&self.mono);
            return;
        }

        let ratio = self.input_rate as f64 / TARGET_SAMPLE_RATE as f64;
        let start_pos = self.src_pos_accum;
        let end_pos = start_pos + (self.mono.len() as f64 / ratio);
        let start_idx = start_pos.ceil() as usize;
        let end_idx = end_pos.ceil() as usize;
        let output_len = end_idx.saturating_sub(start_idx);
        out.reserve(output_len);

        for i in 0..output_len {
            let current_output_idx = (start_idx + i) as f64;
            let src_pos = current_output_idx * ratio - (start_pos * ratio);

            let sidx = src_pos.floor() as isize;
            let frac = (src_pos - sidx as f64) as f32;

            let s0 = if sidx < 0 {
                self.last_sample
            } else {
                self.mono[sidx as usize]
            };
            let s1 = if (sidx + 1) as usize >= self.mono.len() {
                self.mono[self.mono.len() - 1]
            } else {
                self.mono[(sidx + 1) as usize]
            };
            out.push(s0 + frac * (s1 - s0));
        }

        self.src_pos_accum = end_pos % 1.0;
        self.last_sample = self.mono[self.mono.len() - 1];
    }

    #[cfg(test)]
    fn process(&mut self, data: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        self.process_into(data, &mut out);
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
