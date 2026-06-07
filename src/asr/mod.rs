//! Automatic speech recognition — Moonshine Streaming via ONNX.

pub mod engine;

#[cfg(windows)]
mod model_download;
#[cfg(windows)]
mod moonshine;
#[cfg(windows)]
mod worker;

pub use engine::{AsrEngine, AsrError, MockAsrEngine, ModelDownloadError};

#[cfg(windows)]
pub use model_download::{
    ensure_model, model_dir, model_is_complete, DownloadProgress, REQUIRED_MODEL_FILES,
};
#[cfg(windows)]
pub use moonshine::{configure_ort_accelerator, MoonshineEngine};
#[cfg(windows)]
pub use worker::AsrWorker;
