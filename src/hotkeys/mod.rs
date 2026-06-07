pub mod gestures;

#[cfg(windows)]
pub mod hook;

#[cfg(windows)]
pub use hook::{shutdown as shutdown_hotkeys, spawn as spawn_hotkeys};
