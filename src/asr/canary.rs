#[cfg(all(windows, feature = "canary"))]
use std::path::Path;

#[cfg(all(windows, feature = "canary"))]
use tracing::info;
#[cfg(all(windows, feature = "canary"))]
use transcribe_rs::onnx::canary::{CanaryModel, CanaryParams};
#[cfg(all(windows, feature = "canary"))]
use transcribe_rs::onnx::Quantization;

#[cfg(all(windows, feature = "canary"))]
use crate::asr::engine::{AsrEngine, AsrError};
#[cfg(all(windows, feature = "canary"))]
use crate::asr::provision::model_is_complete;
use crate::config::AsrModelId;

#[cfg(all(windows, feature = "canary"))]
pub struct CanaryEngine {
    inner: CanaryModel,
}

#[cfg(all(windows, feature = "canary"))]
impl CanaryEngine {
    pub fn load_from_dir(dir: &Path, threads: usize) -> Result<Self, AsrError> {
        if !model_is_complete(AsrModelId::Canary, dir) {
            return Err(AsrError::Other(format!(
                "Canary model files missing in {}",
                dir.display()
            )));
        }

        info!(
            "Loading Canary model from {} ({} threads)",
            dir.display(),
            threads
        );
        let inner = CanaryModel::load(dir, threads, &Quantization::Int8)
            .map_err(|e| AsrError::Transcription(e.to_string()))?;
        Ok(Self { inner })
    }
}

#[cfg(all(windows, feature = "canary"))]
impl AsrEngine for CanaryEngine {
    fn is_loaded(&self) -> bool {
        true
    }

    fn model_id(&self) -> Option<AsrModelId> {
        Some(AsrModelId::Canary)
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        if samples.is_empty() {
            return Err(AsrError::EmptyAudio);
        }

        let result = self
            .inner
            .transcribe_with(samples, &CanaryParams::default())
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(result.text.trim().to_string())
    }
}
