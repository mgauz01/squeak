#[cfg(all(windows, feature = "parakeet"))]
use std::path::Path;

#[cfg(all(windows, feature = "parakeet"))]
use tracing::info;
#[cfg(all(windows, feature = "parakeet"))]
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
#[cfg(all(windows, feature = "parakeet"))]
use transcribe_rs::onnx::Quantization;

#[cfg(all(windows, feature = "parakeet"))]
use crate::asr::engine::{AsrEngine, AsrError};
#[cfg(all(windows, feature = "parakeet"))]
use crate::asr::provision::model_is_complete;
use crate::config::AsrModelId;

#[cfg(all(windows, feature = "parakeet"))]
pub struct ParakeetEngine {
    inner: ParakeetModel,
}

#[cfg(all(windows, feature = "parakeet"))]
impl ParakeetEngine {
    pub fn load_from_dir(dir: &Path) -> Result<Self, AsrError> {
        if !model_is_complete(AsrModelId::Parakeet, dir) {
            return Err(AsrError::Other(format!(
                "Parakeet model files missing in {}",
                dir.display()
            )));
        }

        info!("Loading Parakeet model from {}", dir.display());
        let inner = ParakeetModel::load(dir, &Quantization::Int8)
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
            .transcribe_with(&samples, &ParakeetParams::default())
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(result.text.trim().to_string())
    }
}
