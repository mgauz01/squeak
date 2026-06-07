//! Squeak — local Windows voice dictation.

pub mod app;
pub mod asr;
pub mod config;
pub mod hotkeys;
pub mod postprocess;

#[cfg(windows)]
pub mod audio;
#[cfg(windows)]
pub mod output;
#[cfg(windows)]
pub mod platform;
#[cfg(windows)]
pub mod ui;

// asr is always available (trait + mock); Moonshine worker is Windows-only.

/// Planning-default gesture timing (may be updated from spike doc).
pub mod timing {
    pub const PTT_MIN_HOLD_MS: u64 = 300;
    pub const DOUBLE_TAP_WINDOW_MS: u64 = 400;
}
