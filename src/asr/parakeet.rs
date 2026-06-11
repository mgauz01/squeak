#[cfg(all(windows, feature = "parakeet"))]
use std::path::Path;

#[cfg(all(windows, feature = "parakeet"))]
use tracing::info;
#[cfg(all(windows, feature = "parakeet"))]
use transcribe_rs::onnx::parakeet::ParakeetModel;
#[cfg(all(windows, feature = "parakeet"))]
use transcribe_rs::onnx::Quantization;
#[cfg(all(windows, feature = "parakeet"))]
use transcribe_rs::{SpeechModel, TranscribeOptions};

#[cfg(all(windows, feature = "parakeet"))]
use crate::asr::engine::{AsrEngine, AsrError};
#[cfg(all(windows, feature = "parakeet"))]
use crate::asr::provision::model_is_complete;
use crate::config::AsrModelId;

/// Leading pad after Squeak trims capture silence — smaller than Parakeet's 250 ms default.
#[cfg(all(windows, feature = "parakeet"))]
const LEADING_SILENCE_MS: u32 = 80;

#[cfg(all(windows, feature = "parakeet"))]
pub struct ParakeetEngine {
    inner: ParakeetModel,
}

#[cfg(all(windows, feature = "parakeet"))]
impl ParakeetEngine {
    pub fn load_from_dir(dir: &Path, threads: usize) -> Result<Self, AsrError> {
        if !model_is_complete(AsrModelId::Parakeet, dir) {
            return Err(AsrError::Other(format!(
                "Parakeet model files missing in {}",
                dir.display()
            )));
        }

        info!(
            "Loading Parakeet model from {} ({} threads)",
            dir.display(),
            threads
        );
        let inner = ParakeetModel::load(dir, threads, &Quantization::Int8)
            .map_err(|e| AsrError::Transcription(e.to_string()))?;
        Ok(Self { inner })
    }
}

#[cfg(all(windows, feature = "parakeet"))]
impl AsrEngine for ParakeetEngine {
    fn is_loaded(&self) -> bool {
        true
    }

    fn model_id(&self) -> Option<AsrModelId> {
        Some(AsrModelId::Parakeet)
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        if samples.is_empty() {
            return Err(AsrError::EmptyAudio);
        }

        let result = self
            .inner
            .transcribe(
                samples,
                &TranscribeOptions {
                    leading_silence_ms: Some(LEADING_SILENCE_MS),
                    ..Default::default()
                },
            )
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(result.text.trim().to_string())
    }
}
