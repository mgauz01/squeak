//! Windows startup folder / Run-key integration for `config.autostart`.

use std::io;
use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Squeak";

fn run_command(program: &str, args: &[&str], err: &str) -> io::Result<()> {
    let status = Command::new(program).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(err))
    }
}

/// Apply the autostart registry value to match `enabled`.
pub fn apply(enabled: bool) -> io::Result<()> {
    if enabled {
        let exe = std::env::current_exe()?;
        let quoted = format!("\"{}\"", exe.display());
        run_command(
            "reg",
            &[
                "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &quoted, "/f",
            ],
            "reg add failed for autostart",
        )
    } else {
        run_command(
            "reg",
            &["delete", RUN_KEY, "/v", VALUE_NAME, "/f"],
            "reg delete failed for autostart",
        )
    }
}

/// Open a file or folder with the shell default handler (`start "" path`).
pub fn open_in_shell(path: &std::path::Path) -> io::Result<()> {
    let path = path.to_string_lossy();
    run_command(
        "cmd",
        &["/C", "start", "", &path],
        "failed to open path in shell",
    )
}
