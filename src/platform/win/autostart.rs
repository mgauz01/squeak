//! Windows startup folder / Run-key integration for `config.autostart`.

use std::io;
use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Squeak";

/// Apply the autostart registry value to match `enabled`.
pub fn apply(enabled: bool) -> io::Result<()> {
    if enabled {
        let exe = std::env::current_exe()?;
        let quoted = format!("\"{}\"", exe.display());
        let status = Command::new("reg")
            .args([
                "add",
                RUN_KEY,
                "/v",
                VALUE_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &quoted,
                "/f",
            ])
            .status()?;
        if !status.success() {
            return Err(io::Error::other("reg add failed for autostart"));
        }
    } else {
        let status = Command::new("reg")
            .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
            .status()?;
        if !status.success() {
            return Err(io::Error::other("reg delete failed for autostart"));
        }
    }
    Ok(())
}

/// Open a file or folder with the shell default handler (`start "" path`).
pub fn open_in_shell(path: &std::path::Path) -> io::Result<()> {
    let path = path.to_string_lossy();
    let status = Command::new("cmd")
        .args(["/C", "start", "", &path])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("failed to open path in shell"))
    }
}
