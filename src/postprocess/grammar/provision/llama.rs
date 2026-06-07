use std::path::Path;

use tracing::info;

use crate::postprocess::grammar::engine::ModelDownloadError;
use crate::postprocess::grammar::provision::{download_hf_files, is_complete_dir, DownloadProgress};

const HF_REPO: &str = "kmaurinjones/grammar-llama-3.2-1B";

pub const LLAMA_REQUIRED_FILES: &[&str] = &["grammar-llama-3.2-1b.Q4_K_M.gguf"];

const HF_FILES: &[(&str, &str)] = &[(
    "grammar-llama-3.2-1b.Q4_K_M.gguf",
    "grammar-llama-3.2-1b.Q4_K_M.gguf",
)];

pub fn is_complete(dir: &Path) -> bool {
    is_complete_dir(dir, LLAMA_REQUIRED_FILES)
}

pub fn download(
    target: &Path,
    progress: &impl Fn(DownloadProgress),
) -> Result<std::path::PathBuf, ModelDownloadError> {
    info!("Fetching grammar-Llama GGUF from HuggingFace ({HF_REPO})");
    progress(DownloadProgress::Extracting);
    download_hf_files(HF_REPO, HF_FILES, target, progress)?;

    if !is_complete(target) {
        return Err(ModelDownloadError::VerificationFailed);
    }

    Ok(target.to_path_buf())
}
