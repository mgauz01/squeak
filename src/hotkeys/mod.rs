pub mod gestures;

#[cfg(windows)]
pub mod hook;

#[cfg(windows)]
pub mod paste_last;

#[cfg(windows)]
pub use hook::{shutdown as shutdown_hotkeys, spawn as spawn_hotkeys};
