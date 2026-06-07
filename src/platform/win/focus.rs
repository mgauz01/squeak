use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    GUITHREADINFO, SetForegroundWindow, ShowWindow, SW_SHOW,
};

/// Window that had keyboard focus when dictation started.
#[derive(Debug, Clone, Copy)]
pub struct FocusTarget(HWND);

impl FocusTarget {
    pub fn hwnd(self) -> HWND {
        self.0
    }

    /// Capture the focused control (or foreground window) at recording start.
    pub fn capture() -> Option<Self> {
        unsafe {
            let foreground = GetForegroundWindow();
            if foreground.0.is_null() {
                return None;
            }

            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            if GetGUIThreadInfo(0, &mut info).0 == 0 {
                return Some(Self(foreground));
            }

            if !info.hwndFocus.0.is_null() {
                Some(Self(info.hwndFocus))
            } else {
                Some(Self(foreground))
            }
        }
    }
}

/// True when the foreground thread reports a focused window (likely text entry).
pub fn has_text_focus() -> bool {
    FocusTarget::capture().is_some_and(|t| !t.hwnd().0.is_null())
}

/// Best-effort restore of the window that was focused when recording began.
pub fn restore_focus(target: FocusTarget) -> bool {
    unsafe {
        let hwnd = target.hwnd();
        if hwnd.0.is_null() {
            return false;
        }

        let foreground = GetForegroundWindow();
        if foreground == hwnd {
            return true;
        }

        let mut fg_pid = Default::default();
        let mut target_pid = Default::default();
        let fg_thread = GetWindowThreadProcessId(foreground, Some(&mut fg_pid));
        let target_thread = GetWindowThreadProcessId(hwnd, Some(&mut target_pid));
        let current_thread = GetCurrentThreadId();

        let attached_fg = fg_thread != 0 && fg_thread != current_thread;
        let attached_target = target_thread != 0 && target_thread != current_thread;

        if attached_fg {
            let _ = AttachThreadInput(current_thread, fg_thread, true);
        }
        if attached_target {
            let _ = AttachThreadInput(current_thread, target_thread, true);
        }

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = BringWindowToTop(hwnd);
        let fg_ok = SetForegroundWindow(hwnd).0 != 0;
        let _ = SetFocus(Some(hwnd));

        if attached_fg {
            let _ = AttachThreadInput(current_thread, fg_thread, false);
        }
        if attached_target {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }

        fg_ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_probe_does_not_panic() {
        let _ = has_text_focus();
        let _ = FocusTarget::capture();
    }
}
