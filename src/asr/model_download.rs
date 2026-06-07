use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use tar::Archive;
use tracing::{info, warn};

use crate::config::{ModelTier, paths};
use crate::asr::engine::ModelDownloadError;

/// Artifacts required in each model directory (see plan U4).
pub const REQUIRED_MODEL_FILES: &[&str] = &[
    "frontend.ort",
    "encoder.ort",
    "adapter.ort",
    "cross_kv.ort",
    "decoder_kv.ort",
    "streaming_config.json",
    "tokenizer.bin",
];

#[derive(Debug, Clone)]
pub enum DownloadProgress {
    Starting { tier: ModelTier },
    Downloading { downloaded: u64, total: Option<u64> },
    Extracting,
    Complete,
}

pub fn model_dir(tier: ModelTier) -> PathBuf {
    paths::model_dir(tier)
}

pub fn model_is_complete(dir: &Path) -> bool {
    dir.is_dir()
        && REQUIRED_MODEL_FILES
            .iter()
            .all(|name| dir.join(name).is_file())
}

/// Download (if needed), extract, and verify the Moonshine model for `tier`.
pub fn ensure_model(
    tier: ModelTier,
    progress: impl Fn(DownloadProgress),
) -> Result<PathBuf, ModelDownloadError> {
    let target = model_dir(tier);
    if model_is_complete(&target) {
        return Ok(target);
    }

    progress(DownloadProgress::Starting { tier });

    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::create_dir_all(target.parent().unwrap_or(Path::new(".")))?;

    let client = Client::builder()
        .user_agent("Squeak/0.1")
        .build()
        .map_err(|e| ModelDownloadError::Http(e.to_string()))?;

    let url = tier.download_url();
    info!("Downloading Moonshine model from {url}");
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| ModelDownloadError::Http(e.to_string()))?;

    if !response.status().is_success() {
        return Err(ModelDownloadError::Http(format!(
            "HTTP {} for {url}",
            response.status()
        )));
    }

    let total = response.content_length();
    let archive_path = target.with_extension("tar.gz.part");
    let mut archive_file = File::create(&archive_path)?;
    let mut downloaded: u64 = 0;
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let read = response
            .read(&mut buffer)
            .map_err(ModelDownloadError::Io)?;
        if read == 0 {
            break;
        }
        archive_file.write_all(&buffer[..read])?;
        downloaded += read as u64;
        progress(DownloadProgress::Downloading {
            downloaded,
            total,
        });
    }
    archive_file.flush()?;
    drop(archive_file);

    progress(DownloadProgress::Extracting);
    extract_tarball(&archive_path, &target)?;

    let _ = fs::remove_file(&archive_path);

    if !model_is_complete(&target) {
        warn!("Model verification failed for {:?}", target);
        let _ = fs::remove_dir_all(&target);
        return Err(ModelDownloadError::VerificationFailed);
    }

    progress(DownloadProgress::Complete);
    Ok(target)
}

fn extract_tarball(archive_path: &Path, target_dir: &Path) -> Result<(), ModelDownloadError> {
    let file = File::open(archive_path)?;
    let decoder = GzDecoder::new(BufReader::new(file));
    let mut archive = Archive::new(decoder);

    let extract_root = target_dir.with_extension("extract-tmp");
    if extract_root.exists() {
        fs::remove_dir_all(&extract_root)?;
    }
    fs::create_dir_all(&extract_root)?;
    archive
        .unpack(&extract_root)
        .map_err(|_| ModelDownloadError::CorruptArchive)?;

    let source_dir = find_model_root(&extract_root).ok_or(ModelDownloadError::CorruptArchive)?;
    fs::create_dir_all(target_dir)?;
    for name in REQUIRED_MODEL_FILES {
        let from = source_dir.join(name);
        if !from.is_file() {
            let _ = fs::remove_dir_all(&extract_root);
            return Err(ModelDownloadError::CorruptArchive);
        }
        fs::copy(&from, target_dir.join(name))?;
    }

    let _ = fs::remove_dir_all(&extract_root);
    Ok(())
}

fn find_model_root(extract_root: &Path) -> Option<PathBuf> {
    if model_is_complete(extract_root) {
        return Some(extract_root.to_path_buf());
    }

    let entries = fs::read_dir(extract_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if model_is_complete(&path) {
                return Some(path);
            }
            if let Some(nested) = find_model_root(&path) {
                return Some(nested);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_file_list_has_seven_entries() {
        assert_eq!(REQUIRED_MODEL_FILES.len(), 7);
    }

    #[test]
    fn incomplete_dir_fails_verification() {
        let dir = std::env::temp_dir().join("squeak-model-test-incomplete");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("streaming_config.json"), b"{}").unwrap();
        assert!(!model_is_complete(&dir));
        let _ = fs::remove_dir_all(&dir);
    }
}
