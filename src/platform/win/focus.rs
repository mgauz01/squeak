use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GUITHREADINFO,
};

/// True when the foreground thread reports a focused window (likely text entry).
pub fn has_text_focus() -> bool {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }

        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(0, &mut info).is_err() {
            return false;
        }

        !info.hwndFocus.0.is_null()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_probe_does_not_panic() {
        let _ = has_text_focus();
    }
}
