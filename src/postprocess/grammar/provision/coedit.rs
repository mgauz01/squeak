use std::path::Path;

use tracing::info;

use crate::postprocess::grammar::engine::ModelDownloadError;
use crate::postprocess::grammar::provision::{
    download_hf_files, is_complete_dir, DownloadProgress,
};

const HF_REPO: &str = "jbochi/coedit-small";

pub const COEDIT_REQUIRED_FILES: &[&str] = &[
    "encoder_model_quantized.onnx",
    "decoder_model_merged_quantized.onnx",
    "tokenizer.json",
    "config.json",
];

const HF_FILES: &[(&str, &str)] = &[
    (
        "onnx/encoder_model_quantized.onnx",
        "encoder_model_quantized.onnx",
    ),
    (
        "onnx/decoder_model_merged_quantized.onnx",
        "decoder_model_merged_quantized.onnx",
    ),
    ("tokenizer.json", "tokenizer.json"),
    ("config.json", "config.json"),
];

pub fn is_complete(dir: &Path) -> bool {
    is_complete_dir(dir, COEDIT_REQUIRED_FILES)
}

pub fn download(
    target: &Path,
    progress: &impl Fn(DownloadProgress),
) -> Result<std::path::PathBuf, ModelDownloadError> {
    info!("Fetching CoEdIT-small from HuggingFace ({HF_REPO})");
    progress(DownloadProgress::Extracting);
    download_hf_files(HF_REPO, HF_FILES, target, progress)?;

    if !is_complete(target) {
        return Err(ModelDownloadError::VerificationFailed);
    }

    Ok(target.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn incomplete_coedit_dir_fails_verification() {
        let dir = std::env::temp_dir().join("squeak-gec-coedit-incomplete");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(!is_complete(&dir));
        let _ = fs::remove_dir_all(&dir);
    }
}
