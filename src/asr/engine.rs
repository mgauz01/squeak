use thiserror::Error;

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("model is downloading; dictation is blocked until complete")]
    Downloading,

    #[error("ASR model is not loaded")]
    NotLoaded,

    #[error("empty audio buffer")]
    EmptyAudio,

    #[error("model download failed: {0}")]
    Download(#[from] ModelDownloadError),

    #[error("transcription failed: {0}")]
    Transcription(String),

    #[error("worker channel closed")]
    WorkerClosed,

    #[error("{0}")]
    Other(String),
}

/// Local speech-to-text engine boundary (v1: Moonshine only).
pub trait AsrEngine: Send {
    fn is_loaded(&self) -> bool;

    fn transcribe(&mut self, samples: &[f32]) -> Result<String, AsrError>;
}

/// Deterministic engine for unit tests and coordinator integration.
pub struct MockAsrEngine {
    loaded: bool,
    response: String,
}

impl MockAsrEngine {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            loaded: true,
            response: response.into(),
        }
    }

    pub fn unloaded() -> Self {
        Self {
            loaded: false,
            response: String::new(),
        }
    }
}

impl AsrEngine for MockAsrEngine {
    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        if !self.loaded {
            return Err(AsrError::NotLoaded);
        }
        if samples.is_empty() {
            return Err(AsrError::EmptyAudio);
        }
        Ok(self.response.clone())
    }
}

#[derive(Debug, Error)]
pub enum ModelDownloadError {
    #[error("HTTP request failed: {0}")]
    Http(String),

    #[error("failed to create model directory: {0}")]
    Io(#[from] std::io::Error),

    #[error("model archive is corrupt or incomplete")]
    CorruptArchive,

    #[error("model verification failed after download")]
    VerificationFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_returns_configured_text() {
        let mut engine = MockAsrEngine::new("hello world");
        let out = engine.transcribe(&[0.1, 0.2]).unwrap();
        assert_eq!(out, "hello world");
    }

    #[test]
    fn mock_rejects_empty_audio() {
        let mut engine = MockAsrEngine::new("x");
        assert!(matches!(
            engine.transcribe(&[]),
            Err(AsrError::EmptyAudio)
        ));
    }

    #[test]
    fn mock_not_loaded_errors() {
        let mut engine = MockAsrEngine::unloaded();
        assert!(matches!(engine.transcribe(&[1.0]), Err(AsrError::NotLoaded)));
    }
}
