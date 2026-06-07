//! Windows-only ASR engine (Moonshine ONNX) — implemented in U4.

#[cfg(windows)]
pub mod engine;
#[cfg(windows)]
pub mod model_download;
#[cfg(windows)]
pub mod moonshine;
