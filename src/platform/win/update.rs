//! Tray-initiated MSI upgrade via GitHub Releases.

use std::fs::{self, File};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;
use windows::core::PCWSTR;
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_YESNO, MESSAGEBOX_STYLE,
};

use crate::platform::win::http;
use crate::update::release::{self, AvailableUpdate, ReleaseError};

pub const GITHUB_RELEASES_LATEST: &str =
    "https://api.github.com/repos/mgauz01/squeak/releases/latest";

const MSI_MAX_BYTES: u64 = 200 * 1024 * 1024;
const API_MAX_BYTES: u64 = 1024 * 1024;

const HELPER_PS1: &str = include_str!("../../../scripts/windows/squeak-update-helper.ps1");

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error(transparent)]
    Release(#[from] ReleaseError),

    #[error("download failed: {0}")]
    Download(#[from] String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// True when the running executable lives under `Program Files\Squeak\`.
pub fn is_msi_install() -> bool {
    std::env::current_exe()
        .map(|path| is_msi_install_path(&path))
        .unwrap_or(false)
}

pub fn is_msi_install_path(exe: &Path) -> bool {
    let normalized = exe.to_string_lossy().replace('/', "\\").to_lowercase();
    normalized.contains("\\program files\\squeak\\")
}

/// Query GitHub for a newer MSI release, if any.
pub fn check_for_upgrade(current_version: &str) -> Result<Option<AvailableUpdate>, UpdateError> {
    let json = http::get_text(GITHUB_RELEASES_LATEST, API_MAX_BYTES)?;
    let update = release::parse_latest_release(&json)?;
    if release::is_newer_than(&update.version, current_version)? {
        Ok(Some(update))
    } else {
        Ok(None)
    }
}

pub fn confirm_update_dialog(current: &str, available: &semver::Version) -> bool {
    let title = "Squeak update available";
    let body = format!(
        "Version {available} is available (you have {current}).\n\n\
         Download and install now? Squeak will close and restart."
    );
    message_box_yes_no(title, &body)
}

pub fn download_msi(url: &str, version: &semver::Version) -> Result<PathBuf, UpdateError> {
    let temp = std::env::temp_dir();
    let dest = temp.join(format!("Squeak-{}-x64.msi.part", version));
    if dest.exists() {
        let _ = fs::remove_file(&dest);
    }

    let mut file = File::create(&dest)?;
    http::stream_url_to_writer(url, &mut file, MSI_MAX_BYTES, |_, _| {})?;
    file.flush()?;
    drop(file);

    let final_path = temp.join(format!("Squeak-{}-x64.msi", version));
    if final_path.exists() {
        let _ = fs::remove_file(&final_path);
    }
    fs::rename(&dest, &final_path)?;
    Ok(final_path)
}

/// Spawn detached elevated installer; caller should exit immediately after.
pub fn launch_upgrade(msi_path: &Path) -> std::io::Result<()> {
    let script_path = std::env::temp_dir().join("squeak-update-helper.ps1");
    fs::write(&script_path, HELPER_PS1)?;

    let msi = msi_path.to_string_lossy();
    let script = script_path.to_string_lossy();

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
            &script,
            "-MsiPath",
            &msi,
        ])
        .creation_flags(CREATE_NO_WINDOW.0 | DETACHED_PROCESS.0)
        .spawn()?;

    drop(status);
    Ok(())
}

fn message_box_yes_no(title: &str, body: &str) -> bool {
    let title = wide_null(title);
    let body = wide_null(body);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MESSAGEBOX_STYLE(MB_YESNO.0 | MB_ICONINFORMATION.0),
        ) == IDYES
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_program_files_install() {
        assert!(is_msi_install_path(Path::new(
            r"C:\Program Files\Squeak\squeak.exe"
        )));
        assert!(is_msi_install_path(Path::new(
            r"C:/Program Files/Squeak/squeak.exe"
        )));
    }

    #[test]
    fn rejects_dev_build_paths() {
        assert!(!is_msi_install_path(Path::new(
            r"C:\Users\dev\projects\squeak\target\release\squeak.exe"
        )));
    }
}
