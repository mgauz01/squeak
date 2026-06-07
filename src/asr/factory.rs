use std::path::Path;

use crate::asr::engine::{AsrEngine, AsrError};
use crate::asr::moonshine::MoonshineEngine;
use crate::config::{AsrModelId, ModelTier};

#[cfg(feature = "parakeet")]
use crate::asr::parakeet::ParakeetEngine;
#[cfg(feature = "cohere")]
use crate::asr::cohere::CohereEngine;
#[cfg(feature = "canary")]
use crate::asr::canary::CanaryEngine;

/// Load the ONNX engine for `model` from `model_dir` (already verified on disk).
#[cfg(windows)]
pub fn create_engine(
    model: AsrModelId,
    model_dir: &Path,
) -> Result<Box<dyn AsrEngine>, AsrError> {
    match model {
        AsrModelId::Moonshine(tier) => {
            Ok(Box::new(MoonshineEngine::load_from_dir(model_dir, tier)?))
        }
        #[cfg(feature = "parakeet")]
        AsrModelId::Parakeet => Ok(Box::new(ParakeetEngine::load_from_dir(model_dir)?)),
        #[cfg(feature = "cohere")]
        AsrModelId::Cohere => Ok(Box::new(CohereEngine::load_from_dir(model_dir)?)),
        #[cfg(feature = "canary")]
        AsrModelId::Canary => Ok(Box::new(CanaryEngine::load_from_dir(model_dir)?)),
    }
}
