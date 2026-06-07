use std::path::Path;

use tracing::info;

use crate::asr::engine::ModelDownloadError;
use crate::asr::provision::moonshine;
use crate::asr::provision::DownloadProgress;

pub const PARAKEET_REQUIRED_FILES: &[&str] = &[
    "nemo128.onnx",
    "vocab.txt",
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
];

pub fn is_complete(dir: &Path) -> bool {
    dir.is_dir()
        && PARAKEET_REQUIRED_FILES
            .iter()
            .all(|name| dir.join(name).is_file())
}

pub fn download_and_extract(
    target: &Path,
    url: &str,
    progress: &impl Fn(DownloadProgress),
) -> Result<std::path::PathBuf, ModelDownloadError> {
    info!("Downloading Parakeet model from {url}");
    moonshine::download_tarball_to_dir(target, url, PARAKEET_REQUIRED_FILES, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn incomplete_parakeet_dir_fails_verification() {
        let dir = std::env::temp_dir().join("squeak-parakeet-test-incomplete");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("vocab.txt"), b"<blk>\n").unwrap();
        assert!(!is_complete(&dir));
        let _ = fs::remove_dir_all(&dir);
    }
}
