//! Text delivery (SendInput, clipboard) — implemented in U8.

#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod delivery;
#[cfg(windows)]
pub mod inject;
