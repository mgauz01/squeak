use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use tar::Archive;
use tracing::{info, warn};

use crate::asr::engine::ModelDownloadError;
use crate::asr::provision::DownloadProgress;
use crate::config::ModelTier;

/// Artifacts required in each Moonshine model directory.
pub const MOONSHINE_REQUIRED_FILES: &[&str] = &[
    "frontend.ort",
    "encoder.ort",
    "adapter.ort",
    "cross_kv.ort",
    "decoder_kv.ort",
    "streaming_config.json",
    "tokenizer.bin",
];

/// Alias kept for existing tests and docs.
pub const REQUIRED_MODEL_FILES: &[&str] = MOONSHINE_REQUIRED_FILES;

pub fn is_complete(dir: &Path, _tier: ModelTier) -> bool {
    is_complete_dir(dir, MOONSHINE_REQUIRED_FILES)
}

pub fn download_and_extract(
    tier: ModelTier,
    target: &Path,
    url: &str,
    progress: &impl Fn(DownloadProgress),
) -> Result<PathBuf, ModelDownloadError> {
    info!("Downloading Moonshine model ({tier:?}) from {url}");
    download_tarball_to_dir(target, url, MOONSHINE_REQUIRED_FILES, progress)
}

pub fn download_tarball_to_dir(
    target: &Path,
    url: &str,
    required_files: &[&str],
    progress: &impl Fn(DownloadProgress),
) -> Result<PathBuf, ModelDownloadError> {
    download_tarball_to_dir_optional(target, url, required_files, &[], progress)
}

pub fn download_tarball_to_dir_optional(
    target: &Path,
    url: &str,
    required_files: &[&str],
    optional_files: &[&str],
    progress: &impl Fn(DownloadProgress),
) -> Result<PathBuf, ModelDownloadError> {
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::create_dir_all(target.parent().unwrap_or(Path::new(".")))?;

    let client = Client::builder()
        .user_agent("Squeak/0.1")
        .build()
        .map_err(|e| ModelDownloadError::Http(e.to_string()))?;

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
    extract_tarball_optional(&archive_path, target, required_files, optional_files)?;

    let _ = fs::remove_file(&archive_path);

    if !is_complete_dir(target, required_files) {
        warn!("Model verification failed for {:?}", target);
        let _ = fs::remove_dir_all(target);
        return Err(ModelDownloadError::VerificationFailed);
    }

    progress(DownloadProgress::Complete);
    Ok(target.to_path_buf())
}

pub fn extract_tarball(
    archive_path: &Path,
    target_dir: &Path,
    required_files: &[&str],
) -> Result<(), ModelDownloadError> {
    extract_tarball_optional(archive_path, target_dir, required_files, &[])
}

pub fn extract_tarball_optional(
    archive_path: &Path,
    target_dir: &Path,
    required_files: &[&str],
    optional_files: &[&str],
) -> Result<(), ModelDownloadError> {
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

    let source_dir =
        find_model_root(&extract_root, required_files).ok_or(ModelDownloadError::CorruptArchive)?;
    fs::create_dir_all(target_dir)?;
    for name in required_files {
        let from = source_dir.join(name);
        if !from.is_file() {
            let _ = fs::remove_dir_all(&extract_root);
            return Err(ModelDownloadError::CorruptArchive);
        }
        fs::copy(&from, target_dir.join(name))?;
    }

    for name in optional_files {
        let from = source_dir.join(name);
        if from.is_file() {
            fs::copy(&from, target_dir.join(name))?;
        }
    }

    let _ = fs::remove_dir_all(&extract_root);
    Ok(())
}

fn find_model_root(extract_root: &Path, required_files: &[&str]) -> Option<PathBuf> {
    if is_complete_dir(extract_root, required_files) {
        return Some(extract_root.to_path_buf());
    }

    let entries = fs::read_dir(extract_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if is_complete_dir(&path, required_files) {
                return Some(path);
            }
            if let Some(nested) = find_model_root(&path, required_files) {
                return Some(nested);
            }
        }
    }
    None
}

fn is_complete_dir(dir: &Path, required_files: &[&str]) -> bool {
    dir.is_dir()
        && required_files
            .iter()
            .all(|name| dir.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_file_list_has_seven_entries() {
        assert_eq!(MOONSHINE_REQUIRED_FILES.len(), 7);
    }

    #[test]
    fn incomplete_dir_fails_verification() {
        let dir = std::env::temp_dir().join("squeak-model-test-incomplete");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("streaming_config.json"), b"{}").unwrap();
        assert!(!is_complete(&dir, ModelTier::Small));
        let _ = fs::remove_dir_all(&dir);
    }
}
