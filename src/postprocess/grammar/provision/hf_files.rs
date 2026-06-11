use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use tracing::info;

use crate::platform::win::http;
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

    let mut total_downloaded: u64 = 0;

    for (remote_path, local_name) in files {
        let url = format!("https://huggingface.co/{repo}/resolve/main/{remote_path}");
        info!("Downloading grammar artifact from {url}");
        let dest = target.join(local_name);
        let mut file = File::create(&dest)?;
        let mut file_downloaded: u64 = 0;

        http::stream_url_to_writer_unlimited(&url, &mut file, |bytes, total| {
            file_downloaded = bytes;
            progress(DownloadProgress::Downloading {
                downloaded: total_downloaded + file_downloaded,
                total,
            });
        })
        .map_err(ModelDownloadError::Http)?;

        file.flush()?;
        total_downloaded += file_downloaded;
    }

    progress(DownloadProgress::Complete);
    Ok(())
}

pub fn is_complete_dir(dir: &Path, required: &[&str]) -> bool {
    dir.is_dir() && required.iter().all(|name| dir.join(name).is_file())
}
