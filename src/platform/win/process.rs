use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Returns the executable file name of the foreground window's process.
pub fn foreground_process_name() -> Option<String> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 512];
        let mut len = buffer.len() as u32;
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
        let _ = CloseHandle(process);

        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_process_name_does_not_panic() {
        let _ = foreground_process_name();
    }
}
