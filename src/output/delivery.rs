use std::sync::Mutex;

use std::thread;
use std::time::Duration;

use thiserror::Error;
use tracing::{info, warn};

use crate::output::clipboard::{self, ClipboardError};
use crate::output::inject::{self, InjectError};
use crate::platform::win::focus::{self, FocusTarget};

const FOCUS_SETTLE_MS: u64 = 20;

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
    /// Transcript saved internally; automatic paste and clipboard both failed.
    SavedOnly,
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

    /// Inject at the captured caret when possible. Always saves `text` for paste-last.
    /// Does not fail after a successful transcript — worst case returns `SavedOnly`.
    pub fn deliver(&self, text: &str, captured_focus: Option<FocusTarget>) -> DeliveryOutcome {
        if text.is_empty() {
            return DeliveryOutcome::CopiedToClipboard;
        }

        // Clipboard+Ctrl+V is O(1) SendInput; unicode inject is O(n) keystrokes and can take seconds.
        let outcome = if try_clipboard_paste(text, captured_focus) {
            DeliveryOutcome::PastedViaClipboard
        } else if try_inject(text, captured_focus) {
            DeliveryOutcome::Injected
        } else {
            warn!("Could not inject at caret; copying transcript to clipboard");
            match clipboard::set_text(text) {
                Ok(()) => {
                    eprintln!(
                        "Could not paste automatically — transcript copied to clipboard (Ctrl+V)."
                    );
                    DeliveryOutcome::CopiedToClipboard
                }
                Err(err) => {
                    eprintln!(
                        "Paste failed ({err}) — press Shift+Alt+Z to retry once focus is in your text field."
                    );
                    DeliveryOutcome::SavedOnly
                }
            }
        };

        *self.last_transcript.lock().unwrap() = Some(text.to_string());
        info!("Transcript delivered ({outcome:?})");
        outcome
    }

    pub fn paste_last(&self) -> Result<DeliveryOutcome, DeliveryError> {
        let text = self
            .last_transcript
            .lock()
            .unwrap()
            .clone()
            .ok_or(DeliveryError::NoLastTranscript)?;

        Ok(self.deliver(&text, FocusTarget::capture()))
    }

    pub fn last_transcript(&self) -> Option<String> {
        self.last_transcript.lock().unwrap().clone()
    }
}

fn settle_focus(target: FocusTarget) -> bool {
    if focus::is_target_focused(target) {
        return true;
    }
    let restored = focus::restore_focus(target);
    if !focus::is_target_focused(target) {
        thread::sleep(Duration::from_millis(FOCUS_SETTLE_MS));
    }
    restored || focus::is_target_focused(target)
}

fn try_inject(text: &str, captured_focus: Option<FocusTarget>) -> bool {
    if let Some(target) = captured_focus {
        let _ = settle_focus(target);
        if inject::inject_unicode(text).is_ok() {
            if focus::is_target_focused(target) {
                return true;
            }
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
        let _ = settle_focus(target);
        if inject::inject_paste().is_ok() && focus::is_target_focused(target) {
            return true;
        }
    }

    if focus::has_text_focus() && inject::inject_paste().is_ok() {
        return true;
    }

    false
}
