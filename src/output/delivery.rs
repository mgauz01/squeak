use std::sync::Mutex;

use thiserror::Error;
use tracing::{info, warn};

use crate::app::DeliveryTarget;
use crate::output::clipboard::{self, ClipboardError};
use crate::output::inject::{self, InjectError};
use crate::platform::win::focus::has_text_focus;

#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("nothing to paste")]
    NoLastTranscript,

    #[error("injection failed: {0}")]
    Inject(#[from] InjectError),

    #[error("clipboard failed: {0}")]
    Clipboard(#[from] ClipboardError),
}

#[derive(Debug)]
pub enum DeliveryOutcome {
    Injected,
    CopiedToClipboard,
    Buffered,
}

pub struct DeliveryChain {
    last_transcript: Mutex<Option<String>>,
}

impl Default for DeliveryChain {
    fn default() -> Self {
        Self::new()
    }
}

impl DeliveryChain {
    pub fn new() -> Self {
        Self {
            last_transcript: Mutex::new(None),
        }
    }

    pub fn choose_target() -> DeliveryTarget {
        if has_text_focus() {
            DeliveryTarget::InjectAtCaret
        } else {
            DeliveryTarget::BufferWithToast
        }
    }

    pub fn deliver(&self, text: &str, target: DeliveryTarget) -> Result<DeliveryOutcome, DeliveryError> {
        if text.is_empty() {
            return Ok(DeliveryOutcome::Buffered);
        }

        let outcome = match target {
            DeliveryTarget::InjectAtCaret => match inject::inject_unicode(text) {
                Ok(()) => DeliveryOutcome::Injected,
                Err(e) => {
                    warn!("SendInput failed ({e}); falling back to clipboard");
                    clipboard::set_text(text)?;
                    DeliveryOutcome::CopiedToClipboard
                }
            },
            DeliveryTarget::ClipboardFallback => {
                clipboard::set_text(text)?;
                DeliveryOutcome::CopiedToClipboard
            }
            DeliveryTarget::BufferWithToast => DeliveryOutcome::Buffered,
        };

        if matches!(outcome, DeliveryOutcome::Buffered) {
            info!("Buffered transcript for unfocused delivery (toast pending U9)");
        }

        *self.last_transcript.lock().unwrap() = Some(text.to_string());
        Ok(outcome)
    }

    pub fn paste_last(&self) -> Result<DeliveryOutcome, DeliveryError> {
        let text = self
            .last_transcript
            .lock()
            .unwrap()
            .clone()
            .ok_or(DeliveryError::NoLastTranscript)?;

        let target = Self::choose_target();
        self.deliver(&text, target)
    }

    pub fn last_transcript(&self) -> Option<String> {
        self.last_transcript.lock().unwrap().clone()
    }
}
