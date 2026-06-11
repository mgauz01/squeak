#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod level_meter;
pub mod normalize;

#[cfg(windows)]
pub use capture::{AudioCapture, AudioError, TARGET_SAMPLE_RATE};
#[cfg(not(windows))]
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
#[cfg(windows)]
pub use level_meter::AudioLevelMeter;
pub use normalize::{
    log_audio_stats, maybe_write_debug_wav, peak_normalize, trim_leading_silence,
    trim_trailing_silence, AudioStats,
};
