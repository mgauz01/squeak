use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetAncestor, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    SetForegroundWindow, ShowWindow, GA_ROOT, GUITHREADINFO, SW_SHOW,
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
            if GetGUIThreadInfo(0, &mut info).is_err() {
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

        // SetForegroundWindow must target a top-level window; hwnd may be an edit child.
        let top_level = GetAncestor(hwnd, GA_ROOT);
        let fg_target = if top_level.0.is_null() {
            hwnd
        } else {
            top_level
        };

        let foreground = GetForegroundWindow();
        if foreground == fg_target || foreground == hwnd {
            let _ = SetFocus(hwnd);
            return true;
        }

        let mut fg_pid = Default::default();
        let mut target_pid = Default::default();
        let fg_thread = GetWindowThreadProcessId(foreground, Some(&mut fg_pid));
        let target_thread = GetWindowThreadProcessId(fg_target, Some(&mut target_pid));
        let current_thread = GetCurrentThreadId();

        let attached_fg = fg_thread != 0 && fg_thread != current_thread;
        let attached_target = target_thread != 0 && target_thread != current_thread;

        if attached_fg {
            let _ = AttachThreadInput(current_thread, fg_thread, true);
        }
        if attached_target {
            let _ = AttachThreadInput(current_thread, target_thread, true);
        }

        let _ = ShowWindow(fg_target, SW_SHOW);
        let _ = BringWindowToTop(fg_target);
        let fg_ok = SetForegroundWindow(fg_target).0 != 0;
        let _ = SetFocus(hwnd);

        if attached_fg {
            let _ = AttachThreadInput(current_thread, fg_thread, false);
        }
        if attached_target {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }

        fg_ok
    }
}

/// True when the captured control (or its root window) currently has keyboard focus.
pub fn is_target_focused(target: FocusTarget) -> bool {
    unsafe {
        let hwnd = target.hwnd();
        if hwnd.0.is_null() {
            return false;
        }

        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(0, &mut info).is_ok() && !info.hwndFocus.0.is_null() {
            if info.hwndFocus == hwnd {
                return true;
            }
            let focus_root = GetAncestor(info.hwndFocus, GA_ROOT);
            let target_root = GetAncestor(hwnd, GA_ROOT);
            if focus_root == target_root {
                return true;
            }
        }

        let foreground = GetForegroundWindow();
        if foreground == hwnd {
            return true;
        }

        let fg_root = GetAncestor(foreground, GA_ROOT);
        let target_root = GetAncestor(hwnd, GA_ROOT);
        fg_root == target_root
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
