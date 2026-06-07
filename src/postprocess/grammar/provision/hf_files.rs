use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use reqwest::blocking::Client;
use tracing::info;

use crate::postprocess::grammar::engine::ModelDownloadError;
use crate::postprocess::grammar::provision::DownloadProgress;

/// Download HuggingFace `resolve/main` files into a flat model directory.
pub fn download_hf_files(
    repo: &str,
    files: &[(&str, &str)],
    target: &Path,
    progress: &impl Fn(DownloadProgress),
) -> Result<(), ModelDownloadError> {
    fs::create_dir_all(target)?;

    let client = Client::builder()
        .user_agent("Squeak/0.1")
        .build()
        .map_err(|e| ModelDownloadError::Http(e.to_string()))?;

    let mut total_downloaded: u64 = 0;

    for (remote_path, local_name) in files {
        let url = format!("https://huggingface.co/{repo}/resolve/main/{remote_path}");
        info!("Downloading grammar artifact from {url}");
        let dest = target.join(local_name);

        let mut response = client
            .get(&url)
            .send()
            .map_err(|e| ModelDownloadError::Http(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ModelDownloadError::Http(format!(
                "HTTP {} for {url}",
                response.status()
            )));
        }

        let total = response.content_length();
        let mut file = File::create(&dest)?;
        let mut buffer = [0u8; 64 * 1024];
        let mut file_downloaded: u64 = 0;

        loop {
            let read = response
                .read(&mut buffer)
                .map_err(ModelDownloadError::Io)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            file_downloaded += read as u64;
            total_downloaded += read as u64;
            progress(DownloadProgress::Downloading {
                downloaded: total_downloaded,
                total,
            });
        }
        file.flush()?;
        let _ = file_downloaded;
    }

    progress(DownloadProgress::Complete);
    Ok(())
}

pub fn is_complete_dir(dir: &Path, required: &[&str]) -> bool {
    dir.is_dir() && required.iter().all(|name| dir.join(name).is_file())
}
