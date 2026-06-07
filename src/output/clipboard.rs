use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("clipboard unavailable: {0}")]
    Failed(String),
}

pub fn set_text(text: &str) -> Result<(), ClipboardError> {
    arboard::Clipboard::new()
        .map_err(|e| ClipboardError::Failed(e.to_string()))?
        .set_text(text.to_string())
        .map_err(|e| ClipboardError::Failed(e.to_string()))
}

pub fn get_text() -> Result<String, ClipboardError> {
    arboard::Clipboard::new()
        .map_err(|e| ClipboardError::Failed(e.to_string()))?
        .get_text()
        .map_err(|e| ClipboardError::Failed(e.to_string()))
}
