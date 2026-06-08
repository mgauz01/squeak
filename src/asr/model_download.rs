//! Backward-compatible re-exports — prefer `crate::asr::provision`.

use std::path::Path;

use crate::config::{AsrModelId, ModelTier};

pub use crate::asr::provision::{ensure_model, model_dir, DownloadProgress, REQUIRED_MODEL_FILES};

pub fn model_is_complete(model: AsrModelId, dir: &Path) -> bool {
    crate::asr::provision::model_is_complete(model, dir)
}

/// Moonshine-only helper for legacy callers.
pub fn moonshine_dir_is_complete(dir: &Path, tier: ModelTier) -> bool {
    model_is_complete(AsrModelId::moonshine(tier), dir)
}
