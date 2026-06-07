//! Model download and verification per ASR backend.

mod moonshine;

#[cfg(feature = "parakeet")]
mod parakeet;

#[cfg(feature = "cohere")]
mod cohere;

#[cfg(feature = "canary")]
mod canary;

use std::path::{Path, PathBuf};

use crate::asr::engine::ModelDownloadError;
use crate::config::{model_dir_for, AsrModelId, ModelTier};

pub use moonshine::REQUIRED_MODEL_FILES;

#[derive(Debug, Clone)]
pub enum DownloadProgress {
    Starting { model: AsrModelId },
    Downloading {
        downloaded: u64,
        total: Option<u64>,
    },
    Extracting,
    Complete,
}

pub fn model_dir(model: AsrModelId) -> PathBuf {
    model_dir_for(model)
}

pub fn model_is_complete(model: AsrModelId, dir: &Path) -> bool {
    match model {
        AsrModelId::Moonshine(tier) => moonshine::is_complete(dir, tier),
        #[cfg(feature = "parakeet")]
        AsrModelId::Parakeet => parakeet::is_complete(dir),
        #[cfg(feature = "cohere")]
        AsrModelId::Cohere => cohere::is_complete(dir),
        #[cfg(feature = "canary")]
        AsrModelId::Canary => canary::is_complete(dir),
    }
}

/// Backward-compatible Moonshine-only check.
pub fn moonshine_model_is_complete(dir: &Path) -> bool {
    moonshine::is_complete(dir, ModelTier::Small)
        || moonshine::is_complete(dir, ModelTier::Tiny)
        || moonshine::is_complete(dir, ModelTier::Medium)
}

pub fn ensure_model(
    model: AsrModelId,
    progress: impl Fn(DownloadProgress),
) -> Result<PathBuf, ModelDownloadError> {
    let target = model_dir(model);
    if model_is_complete(model, &target) {
        return Ok(target);
    }

    let url = model.download_url().ok_or(ModelDownloadError::Http(
        format!("no download URL for {model:?}"),
    ))?;

    progress(DownloadProgress::Starting { model });

    match model {
        AsrModelId::Moonshine(tier) => {
            moonshine::download_and_extract(tier, &target, url, &progress)
        }
        #[cfg(feature = "parakeet")]
        AsrModelId::Parakeet => parakeet::download_and_extract(&target, url, &progress),
        #[cfg(feature = "cohere")]
        AsrModelId::Cohere => cohere::download_and_extract(&target, url, &progress),
        #[cfg(feature = "canary")]
        AsrModelId::Canary => canary::download_and_extract(&target, url, &progress),
    }
}
