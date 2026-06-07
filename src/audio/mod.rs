pub mod capture;
pub mod normalize;

pub use capture::{AudioCapture, AudioError, TARGET_SAMPLE_RATE};
pub use normalize::{log_audio_stats, maybe_write_debug_wav, peak_normalize, AudioStats};
