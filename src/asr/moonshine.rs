//! Moonshine Streaming ASR via ONNX (`transcribe-rs`).
//!
//! The streaming frontend expects fixed **1280-sample** chunks (`CHUNK_SIZE` in
//! `transcribe-rs`). Partial tail chunks crash the Conv node unless padded to
//! 1280 with silence before inference.
use std::borrow::Cow;
use std::path::Path;

use tracing::{info, warn};
use transcribe_rs::onnx::moonshine::StreamingModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::{SpeechModel, TranscribeOptions};

use crate::asr::engine::{recommended_thread_count, AsrEngine, AsrError};
use crate::asr::provision::{model_dir, model_is_complete};
use crate::config::{AsrModelId, ModelTier};

/// Moonshine streaming frontend processes fixed 1280-sample chunks (transcribe-rs).
const STREAMING_CHUNK_SAMPLES: usize = 1280;

pub struct MoonshineEngine {
    inner: StreamingModel,
    tier: ModelTier,
    model: AsrModelId,
}

impl MoonshineEngine {
    pub fn load(model: AsrModelId) -> Result<Self, AsrError> {
        let tier = model.moonshine_tier().ok_or_else(|| {
            AsrError::Other(format!("not a Moonshine model: {}", model.config_key()))
        })?;
        let dir = model_dir(model);
        Self::load_from_dir(&dir, tier)
    }

    pub fn load_from_dir(dir: &Path, tier: ModelTier) -> Result<Self, AsrError> {
        let model = AsrModelId::moonshine(tier);
        if !model_is_complete(model, dir) {
            return Err(AsrError::Other(format!(
                "model files missing in {}",
                dir.display()
            )));
        }

        info!("Loading Moonshine streaming model from {}", dir.display());
        let threads = recommended_thread_count();
        let inner = StreamingModel::load(dir, threads, &Quantization::default())
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(Self { inner, tier, model })
    }

    pub fn tier(&self) -> ModelTier {
        self.tier
    }

    pub fn model_id(&self) -> AsrModelId {
        self.model
    }
}

impl AsrEngine for MoonshineEngine {
    fn is_loaded(&self) -> bool {
        true
    }

    fn model_id(&self) -> Option<AsrModelId> {
        Some(self.model)
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        if samples.is_empty() {
            return Err(AsrError::EmptyAudio);
        }
        if samples.len() < STREAMING_CHUNK_SAMPLES {
            return Err(AsrError::AudioTooShort {
                samples: samples.len(),
                min: STREAMING_CHUNK_SAMPLES,
            });
        }

        let padded = pad_to_streaming_chunks(samples);
        info!(
            "Transcribing {} samples ({} after chunk padding)",
            samples.len(),
            padded.len()
        );

        let result = self
            .inner
            .transcribe(&padded, &TranscribeOptions::default())
            .map_err(|e| AsrError::Transcription(e.to_string()))?;

        Ok(result.text.trim().to_string())
    }
}

/// Partial final chunks (e.g. len % 1280 == 4) crash the Moonshine frontend Conv node.
///
/// Returns a `Cow` to avoid cloning when the input is already a multiple of 1280.
fn pad_to_streaming_chunks(samples: &[f32]) -> Cow<'_, [f32]> {
    let rem = samples.len() % STREAMING_CHUNK_SAMPLES;
    if rem == 0 {
        return Cow::Borrowed(samples);
    }
    let mut padded = samples.to_vec();
    padded.resize(samples.len() + STREAMING_CHUNK_SAMPLES - rem, 0.0);
    Cow::Owned(padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_partial_final_chunk_to_multiple_of_1280() {
        let input = vec![0.0; 1284];
        let padded = pad_to_streaming_chunks(&input);
        assert_eq!(padded.len(), 2560);
        assert_eq!(&padded[..1284], &input[..]);
        assert!(padded[1284..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn exact_chunk_multiple_unchanged() {
        let input = vec![0.5; 2560];
        let padded = pad_to_streaming_chunks(&input);
        assert_eq!(padded, input);
    }

    #[test]
    fn single_tail_pad_only() {
        let input = vec![1.0; 1284];
        let padded = pad_to_streaming_chunks(&input);
        assert_eq!(padded.len(), 2560);
        assert_eq!(&padded[..1284], &input[..]);
        assert!(padded[1284..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn prebuilt_ort_does_not_offer_xnnpack() {
        assert!(!xnnpack_available());
    }

    #[test]
    fn detects_directml_slice_errors() {
        assert!(is_likely_directml_inference_error(
            "inference error: Non-zero status code returned while running Slice node. 80070057"
        ));
        assert!(!is_likely_directml_inference_error("recording too short"));
    }
}

/// Whether XNNPACK can actually be used in this binary.
///
/// The `ort/xnnpack` crate feature only enables registration APIs. Microsoft's
/// prebuilt ORT DLLs from `download-binaries` do not ship the XNNPACK provider,
/// which produces `XnnpackExecutionProvider is not supported in this build` errors.
pub fn xnnpack_available() -> bool {
    false
}

/// Configure ORT execution provider and CPU thread budget before loading ONNX sessions.
///
/// Microsoft's ORT prebuilds use OpenMP, so Parakeet/Cohere/Canary benefit from
/// `OMP_NUM_THREADS` even though their sessions are created without `with_intra_threads`.
pub fn configure_ort_runtime(
    model: AsrModelId,
    prefer_directml: bool,
    threads: usize,
    use_xnnpack: bool,
) {
    let threads = threads.max(1);
    // SAFETY: called on the ASR worker thread before any ORT sessions exist.
    unsafe { std::env::set_var("OMP_NUM_THREADS", threads.to_string()) };

    let use_directml = prefer_directml && model.compatible_with_directml();
    if prefer_directml && !model.compatible_with_directml() {
        warn!(
            "DirectML is not compatible with {} (ONNX Slice ops fail on DML); using CPU",
            model.config_key()
        );
    }

    if use_directml {
        #[cfg(feature = "directml")]
        {
            use transcribe_rs::{set_ort_accelerator, OrtAccelerator};
            set_ort_accelerator(OrtAccelerator::DirectMl);
            info!(
                "ORT accelerator for {}: DirectML ({} threads)",
                model.config_key(),
                threads
            );
            return;
        }
        #[cfg(not(feature = "directml"))]
        warn!("DirectML requested but squeak was built without `directml` feature; using CPU");
    }

    #[cfg(feature = "xnnpack")]
    if use_xnnpack {
        if xnnpack_available() {
            use transcribe_rs::{set_ort_accelerator, OrtAccelerator};
            set_ort_accelerator(OrtAccelerator::Xnnpack);
            info!(
                "ORT accelerator for {}: XNNPACK ({} threads)",
                model.config_key(),
                threads
            );
            return;
        }
        warn!(
            "config xnnpack=true ignored: prebuilt ONNX Runtime has no XNNPACK provider; using CPU ({} threads)",
            threads
        );
    }

    use transcribe_rs::{set_ort_accelerator, OrtAccelerator};
    set_ort_accelerator(OrtAccelerator::CpuOnly);
    info!(
        "ORT accelerator for {}: CPU ({} threads)",
        model.config_key(),
        threads
    );
}

pub fn ort_accelerator_summary(
    model: AsrModelId,
    prefer_directml: bool,
    use_xnnpack: bool,
) -> &'static str {
    if prefer_directml && model.compatible_with_directml() {
        #[cfg(feature = "directml")]
        {
            return "DirectML";
        }
    }
    #[cfg(feature = "xnnpack")]
    if use_xnnpack && xnnpack_available() {
        return "XNNPACK";
    }
    "CPU"
}

/// True when a transcription error likely came from the DirectML execution provider.
pub fn is_likely_directml_inference_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("80070057")
        || lower.contains("e_invalidarg")
        || lower.contains("dml")
        || lower.contains("directml")
        || (lower.contains("slice") && lower.contains("inference"))
}
