//! Automatic speech recognition — swappable local backends via ONNX.

pub mod engine;

#[cfg(all(windows, feature = "canary"))]
mod canary;
#[cfg(all(windows, feature = "cohere"))]
mod cohere;
#[cfg(windows)]
mod factory;
#[cfg(windows)]
mod model_download;
#[cfg(windows)]
mod moonshine;
#[cfg(all(windows, feature = "parakeet"))]
mod parakeet;
#[cfg(windows)]
mod provision;
#[cfg(windows)]
mod worker;

pub use engine::{
    recommended_thread_count, AsrEngine, AsrError, MockAsrEngine, ModelDownloadError,
};

#[cfg(all(windows, feature = "canary"))]
pub use canary::CanaryEngine;
#[cfg(all(windows, feature = "cohere"))]
pub use cohere::CohereEngine;
#[cfg(windows)]
pub use model_download::{
    ensure_model, model_dir, model_is_complete, DownloadProgress, REQUIRED_MODEL_FILES,
};
#[cfg(windows)]
pub use moonshine::{
    configure_ort_runtime, is_likely_directml_inference_error, ort_accelerator_summary,
    xnnpack_available, MoonshineEngine,
};
#[cfg(all(windows, feature = "parakeet"))]
pub use parakeet::ParakeetEngine;
#[cfg(windows)]
pub use worker::{AsrWorker, AsrWorkerConfig};
