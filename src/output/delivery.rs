use std::sync::Mutex;

use std::thread;
use std::time::Duration;

use thiserror::Error;
use tracing::{info, warn};

use crate::output::clipboard::{self, ClipboardError};
use crate::output::inject::{self, InjectError};
use crate::platform::win::focus::{self, FocusTarget};

const FOCUS_SETTLE_MS: u64 = 50;

#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("nothing to paste")]
    NoLastTranscript,

    #[error("injection failed: {0}")]
    Inject(#[from] InjectError),

    #[error("clipboard failed: {0}")]
    Clipboard(#[from] ClipboardError),
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Injected,
    PastedViaClipboard,
    CopiedToClipboard,
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

    pub fn deliver(
        &self,
        text: &str,
        captured_focus: Option<FocusTarget>,
    ) -> Result<DeliveryOutcome, DeliveryError> {
        if text.is_empty() {
            return Ok(DeliveryOutcome::CopiedToClipboard);
        }

        let outcome = if try_inject(text, captured_focus) {
            DeliveryOutcome::Injected
        } else if try_clipboard_paste(text, captured_focus) {
            DeliveryOutcome::PastedViaClipboard
        } else {
            warn!("Could not inject at caret; copying transcript to clipboard");
            clipboard::set_text(text)?;
            eprintln!("Transcript copied to clipboard — press Ctrl+V to paste.");
            DeliveryOutcome::CopiedToClipboard
        };

        *self.last_transcript.lock().unwrap() = Some(text.to_string());
        info!("Transcript delivered ({outcome:?})");
        Ok(outcome)
    }

    pub fn paste_last(&self) -> Result<DeliveryOutcome, DeliveryError> {
        let text = self
            .last_transcript
            .lock()
            .unwrap()
            .clone()
            .ok_or(DeliveryError::NoLastTranscript)?;

        self.deliver(&text, FocusTarget::capture())
    }

    pub fn last_transcript(&self) -> Option<String> {
        self.last_transcript.lock().unwrap().clone()
    }
}

fn try_inject(text: &str, captured_focus: Option<FocusTarget>) -> bool {
    if let Some(target) = captured_focus {
        let _ = focus::restore_focus(target);
        thread::sleep(Duration::from_millis(FOCUS_SETTLE_MS));
        if focus::is_target_focused(target) && inject::inject_unicode(text).is_ok() {
            return true;
        }
    }

    if focus::has_text_focus() && inject::inject_unicode(text).is_ok() {
        return true;
    }

    false
}

fn try_clipboard_paste(text: &str, captured_focus: Option<FocusTarget>) -> bool {
    if clipboard::set_text(text).is_err() {
        return false;
    }

    if let Some(target) = captured_focus {
        let _ = focus::restore_focus(target);
        thread::sleep(Duration::from_millis(FOCUS_SETTLE_MS));
        if focus::is_target_focused(target) && inject::inject_paste().is_ok() {
            return true;
        }
    }

    if focus::has_text_focus() && inject::inject_paste().is_ok() {
        return true;
    }

    false
}
