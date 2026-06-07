use std::path::Path;
use std::thread;

use tracing::{info, warn};
use transcribe_rs::onnx::moonshine::StreamingModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::{SpeechModel, TranscribeOptions};

use crate::asr::engine::{AsrEngine, AsrError};
use crate::asr::model_download::{model_dir, model_is_complete};
use crate::config::ModelTier;

const DEFAULT_THREADS: usize = 4;
/// Moonshine streaming frontend processes fixed 1280-sample chunks (transcribe-rs).
const STREAMING_CHUNK_SAMPLES: usize = 1280;

pub struct MoonshineEngine {
    inner: StreamingModel,
    tier: ModelTier,
}

impl MoonshineEngine {
    pub fn load(tier: ModelTier) -> Result<Self, AsrError> {
        let dir = model_dir(tier);
        if !model_is_complete(&dir) {
            return Err(AsrError::Other(format!(
                "model files missing in {}",
                dir.display()
            )));
        }

        info!("Loading Moonshine streaming model from {}", dir.display());
        let inner = StreamingModel::load(&dir, DEFAULT_THREADS, &Quantization::default())
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(Self { inner, tier })
    }

    pub fn tier(&self) -> ModelTier {
        self.tier
    }

    pub fn load_from_dir(dir: &Path, tier: ModelTier) -> Result<Self, AsrError> {
        if !model_is_complete(dir) {
            return Err(AsrError::Other(format!(
                "model files missing in {}",
                dir.display()
            )));
        }
        let inner = StreamingModel::load(dir, DEFAULT_THREADS, &Quantization::default())
            .map_err(|e| AsrError::Transcription(e.to_string()))?;
        Ok(Self { inner, tier })
    }
}

impl AsrEngine for MoonshineEngine {
    fn is_loaded(&self) -> bool {
        true
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        if samples.is_empty() {
            return Err(AsrError::EmptyAudio);
        }
        if samples.len() < STREAMING_CHUNK_SAMPLES {
            return Err(AsrError::AudioTooShort {
                samples: samples.len(),
                min: STREAMING_CHUNK_SAMPLES,
            });
        }

        let padded = pad_to_streaming_chunks(samples);
        info!(
            "Transcribing {} samples ({} after chunk padding)",
            samples.len(),
            padded.len()
        );

        let result = self
            .inner
            .transcribe(&padded, &TranscribeOptions::default())
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(result.text.trim().to_string())
    }
}

/// Partial final chunks (e.g. len % 1280 == 4) crash the Moonshine frontend Conv node.
fn pad_to_streaming_chunks(samples: &[f32]) -> Vec<f32> {
    let rem = samples.len() % STREAMING_CHUNK_SAMPLES;
    if rem == 0 {
        return samples.to_vec();
    }
    let mut padded = samples.to_vec();
    padded.resize(samples.len() + STREAMING_CHUNK_SAMPLES - rem, 0.0);
    padded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_partial_final_chunk_to_multiple_of_1280() {
        let input = vec![0.0; 1284];
        let padded = pad_to_streaming_chunks(&input);
        assert_eq!(padded.len(), 2560);
        assert_eq!(&padded[..1284], &input[..]);
        assert!(padded[1284..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn exact_chunk_multiple_unchanged() {
        let input = vec![0.5; 2560];
        let padded = pad_to_streaming_chunks(&input);
        assert_eq!(padded, input);
    }
}

/// Apply ORT accelerator preference before any model load (call once at startup).
pub fn configure_ort_accelerator(use_directml: bool) {
    use transcribe_rs::{set_ort_accelerator, OrtAccelerator};

    if use_directml {
        #[cfg(feature = "directml")]
        {
            set_ort_accelerator(OrtAccelerator::DirectMl);
            info!("ORT accelerator: DirectML");
            return;
        }
        #[cfg(not(feature = "directml"))]
        warn!("DirectML requested but squeak was built without `directml` feature; using CPU");
    }

    set_ort_accelerator(OrtAccelerator::CpuOnly);
    info!("ORT accelerator: CPU");
}

pub fn recommended_thread_count() -> usize {
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(DEFAULT_THREADS)
        .clamp(1, 8)
}
