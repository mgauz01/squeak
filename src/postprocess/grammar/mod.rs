//! Optional local grammar correction (GEC) backends.

pub mod engine;

#[cfg(windows)]
pub mod worker;

#[cfg(all(windows, any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama")))]
mod factory;
#[cfg(all(windows, any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama")))]
mod provision;
#[cfg(all(windows, any(feature = "gec-tiny", feature = "gec-coedit")))]
mod t5_onnx;
#[cfg(all(windows, feature = "gec-tiny"))]
mod tiny_t5;
#[cfg(all(windows, feature = "gec-coedit"))]
mod coedit;
#[cfg(all(windows, feature = "gec-llama"))]
mod llama;

pub use engine::MockGrammarPolisher;

#[cfg(windows)]
pub use worker::GrammarWorker;
