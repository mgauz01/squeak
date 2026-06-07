use thiserror::Error;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    CF_UNICODETEXT,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("clipboard unavailable: {0}")]
    Failed(String),
}

pub fn set_text(text: &str) -> Result<(), ClipboardError> {
    let wide = text_to_wide(text);
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        open_clipboard()?;
        if EmptyClipboard().is_err() {
            let _ = CloseClipboard();
            return Err(ClipboardError::Failed("EmptyClipboard failed".into()));
        }

        let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len)
            .map_err(|e| clipboard_err("GlobalAlloc", e))?;
        let locked = GlobalLock(handle);
        if locked.is_null() {
            let _ = GlobalFree(handle);
            let _ = CloseClipboard();
            return Err(ClipboardError::Failed("GlobalLock failed".into()));
        }

        std::ptr::copy_nonoverlapping(wide.as_ptr(), locked as *mut u16, wide.len());
        let _ = GlobalUnlock(handle);

        if SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(handle.0)).is_err() {
            let _ = GlobalFree(handle);
            let _ = CloseClipboard();
            return Err(ClipboardError::Failed("SetClipboardData failed".into()));
        }

        CloseClipboard()
            .map_err(|e| clipboard_err("CloseClipboard", e))?;
    }

    Ok(())
}

pub fn get_text() -> Result<String, ClipboardError> {
    unsafe {
        open_clipboard()?;
        let handle = GetClipboardData(CF_UNICODETEXT.0 as u32)
            .map_err(|e| clipboard_err("GetClipboardData", e))?;
        if handle.0 == 0 {
            let _ = CloseClipboard();
            return Err(ClipboardError::Failed("clipboard empty".into()));
        }

        let locked = GlobalLock(handle);
        if locked.is_null() {
            let _ = CloseClipboard();
            return Err(ClipboardError::Failed("GlobalLock failed".into()));
        }

        let mut len = 0usize;
        let mut cursor = locked as *const u16;
        while *cursor.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(cursor, len);
        let text = String::from_utf16(slice)
            .map_err(|e| ClipboardError::Failed(format!("invalid UTF-16 in clipboard: {e}")))?;

        let _ = GlobalUnlock(handle);
        CloseClipboard()
            .map_err(|e| clipboard_err("CloseClipboard", e))?;
        Ok(text)
    }
}

fn text_to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn open_clipboard() -> Result<(), ClipboardError> {
    OpenClipboard(HWND::default())
        .map_err(|e| clipboard_err("OpenClipboard", e))
}

fn clipboard_err(op: &str, err: windows::core::Error) -> ClipboardError {
    ClipboardError::Failed(format!("{op} failed: {err}"))
}
