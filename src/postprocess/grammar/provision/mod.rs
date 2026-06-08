//! Grammar model download and verification.

#[cfg(feature = "gec-tiny")]
mod tiny_t5;

#[cfg(feature = "gec-coedit")]
mod coedit;

#[cfg(feature = "gec-llama")]
mod llama;

mod hf_files;

use std::path::{Path, PathBuf};

use crate::config::{grammar_model_dir_for, GrammarModelId};
use crate::postprocess::grammar::engine::ModelDownloadError;

pub use hf_files::download_hf_files;

#[derive(Debug, Clone)]
pub enum DownloadProgress {
    Starting { model: GrammarModelId },
    Downloading { downloaded: u64, total: Option<u64> },
    Extracting,
    Complete,
}

pub fn model_dir(model: GrammarModelId) -> PathBuf {
    grammar_model_dir_for(model)
}

pub fn model_is_complete(model: GrammarModelId, dir: &Path) -> bool {
    match model {
        GrammarModelId::Tiny => {
            #[cfg(feature = "gec-tiny")]
            {
                tiny_t5::is_complete(dir)
            }
            #[cfg(not(feature = "gec-tiny"))]
            {
                let _ = dir;
                false
            }
        }
        #[cfg(feature = "gec-coedit")]
        GrammarModelId::Coedit => coedit::is_complete(dir),
        #[cfg(feature = "gec-llama")]
        GrammarModelId::Llama => llama::is_complete(dir),
    }
}

pub fn ensure_model(
    model: GrammarModelId,
    progress: impl Fn(DownloadProgress),
) -> Result<PathBuf, ModelDownloadError> {
    let target = model_dir(model);
    if model_is_complete(model, &target) {
        return Ok(target);
    }

    progress(DownloadProgress::Starting { model });

    match model {
        GrammarModelId::Tiny => {
            #[cfg(feature = "gec-tiny")]
            {
                tiny_t5::download(&target, &progress)
            }
            #[cfg(not(feature = "gec-tiny"))]
            {
                let _ = progress;
                Err(ModelDownloadError::Http(
                    "gec-tiny feature not enabled".into(),
                ))
            }
        }
        #[cfg(feature = "gec-coedit")]
        GrammarModelId::Coedit => coedit::download(&target, &progress),
        #[cfg(feature = "gec-llama")]
        GrammarModelId::Llama => llama::download(&target, &progress),
    }
}
