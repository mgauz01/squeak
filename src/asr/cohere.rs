#[cfg(all(windows, feature = "cohere"))]
use std::path::Path;

#[cfg(all(windows, feature = "cohere"))]
use tracing::info;
#[cfg(all(windows, feature = "cohere"))]
use transcribe_rs::onnx::cohere::{CohereModel, CohereParams};
#[cfg(all(windows, feature = "cohere"))]
use transcribe_rs::onnx::Quantization;

#[cfg(all(windows, feature = "cohere"))]
use crate::asr::engine::{AsrEngine, AsrError};
#[cfg(all(windows, feature = "cohere"))]
use crate::asr::provision::model_is_complete;
use crate::config::AsrModelId;

#[cfg(all(windows, feature = "cohere"))]
pub struct CohereEngine {
    inner: CohereModel,
}

#[cfg(all(windows, feature = "cohere"))]
impl CohereEngine {
    pub fn load_from_dir(dir: &Path) -> Result<Self, AsrError> {
        if !model_is_complete(AsrModelId::Cohere, dir) {
            return Err(AsrError::Other(format!(
                "Cohere model files missing in {}",
                dir.display()
            )));
        }

        info!("Loading Cohere model from {}", dir.display());
        let inner = CohereModel::load(dir, &Quantization::Int8)
            .map_err(|e| AsrError::Transcription(e.to_string()))?;
        Ok(Self { inner })
    }
}

#[cfg(all(windows, feature = "cohere"))]
impl AsrEngine for CohereEngine {
    fn is_loaded(&self) -> bool {
        true
    }

    fn model_id(&self) -> Option<AsrModelId> {
        Some(AsrModelId::Cohere)
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        if samples.is_empty() {
            return Err(AsrError::EmptyAudio);
        }

        let result = self
            .inner
            .transcribe_with(
                samples,
                &CohereParams {
                    language: Some("en".into()),
                    ..Default::default()
                },
            )
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(result.text.trim().to_string())
    }
}
