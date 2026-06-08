use thiserror::Error;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
    VK_CONTROL, VK_V,
};

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("SendInput rejected input (UIPI or focus issue)")]
    Rejected,

    #[error("SendInput failed: {0}")]
    Win32(String),
}

pub fn inject_unicode(text: &str) -> Result<(), InjectError> {
    if text.is_empty() {
        return Ok(());
    }

    let inputs: Vec<INPUT> = text
        .chars()
        .flat_map(|ch| {
            [
                make_unicode_key(ch, KEYEVENTF_UNICODE),
                make_unicode_key(ch, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(InjectError::Rejected);
    }
    Ok(())
}

/// Simulate Ctrl+V to paste from the clipboard into the focused control.
pub fn inject_paste() -> Result<(), InjectError> {
    let inputs = [
        make_vk_key(VK_CONTROL, Default::default()),
        make_vk_key(VK_V, Default::default()),
        make_vk_key(VK_V, KEYEVENTF_KEYUP),
        make_vk_key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(InjectError::Rejected);
    }
    Ok(())
}

fn make_vk_key(
    vk: VIRTUAL_KEY,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> INPUT {
    INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn make_unicode_key(
    ch: char,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> INPUT {
    INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: ch as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
