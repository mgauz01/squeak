use std::path::Path;

use tracing::info;

use crate::asr::engine::ModelDownloadError;
use crate::asr::provision::moonshine;
use crate::asr::provision::DownloadProgress;

pub const COHERE_REQUIRED_FILES: &[&str] = &[
    "cohere-encoder.int8.onnx",
    "cohere-decoder.int8.onnx",
    "tokens.txt",
];

/// External weight sidecars (present in some packaged tarballs).
pub const COHERE_OPTIONAL_FILES: &[&str] = &[
    "cohere-encoder.int8.onnx.data",
    "cohere-decoder.int8.onnx.data",
];

pub fn is_complete(dir: &Path) -> bool {
    dir.is_dir()
        && COHERE_REQUIRED_FILES
            .iter()
            .all(|name| dir.join(name).is_file())
}

pub fn download_and_extract(
    target: &Path,
    url: &str,
    progress: &impl Fn(DownloadProgress),
) -> Result<std::path::PathBuf, ModelDownloadError> {
    info!("Downloading Cohere model from {url}");
    moonshine::download_tarball_to_dir_optional(
        target,
        url,
        COHERE_REQUIRED_FILES,
        COHERE_OPTIONAL_FILES,
        progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn incomplete_cohere_dir_fails_verification() {
        let dir = std::env::temp_dir().join("squeak-cohere-test-incomplete");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tokens.txt"), b"<s>\n").unwrap();
        assert!(!is_complete(&dir));
        let _ = fs::remove_dir_all(&dir);
    }
}
