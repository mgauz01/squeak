use thiserror::Error;

use crate::config::GrammarModelId;

#[derive(Debug, Error)]
pub enum GrammarError {
    #[error("grammar model is downloading; polish blocked until complete")]
    Downloading,

    #[error("grammar model is not loaded")]
    NotLoaded,

    #[error("empty text")]
    EmptyText,

    #[error("model download failed: {0}")]
    Download(#[from] ModelDownloadError),

    #[error("grammar correction failed: {0}")]
    Polish(String),

    #[error("worker channel closed")]
    WorkerClosed,

    #[error("grammar backend not available in this build")]
    Unavailable,

    #[error("{0}")]
    Other(String),
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

/// Local grammar correction boundary (mirrors `AsrEngine`).
pub trait GrammarPolisher: Send {
    fn is_loaded(&self) -> bool;

    fn model_id(&self) -> Option<GrammarModelId> {
        None
    }

    fn polish(&mut self, text: &str) -> Result<String, GrammarError>;
}

/// Deterministic polisher for unit tests.
pub struct MockGrammarPolisher {
    loaded: bool,
    suffix: Option<String>,
    fail: bool,
}

impl MockGrammarPolisher {
    pub fn identity() -> Self {
        Self {
            loaded: true,
            suffix: None,
            fail: false,
        }
    }

    pub fn with_suffix(suffix: impl Into<String>) -> Self {
        Self {
            loaded: true,
            suffix: Some(suffix.into()),
            fail: false,
        }
    }

    pub fn failing() -> Self {
        Self {
            loaded: true,
            suffix: None,
            fail: true,
        }
    }
}

impl GrammarPolisher for MockGrammarPolisher {
    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn polish(&mut self, text: &str) -> Result<String, GrammarError> {
        if self.fail {
            return Err(GrammarError::Polish("mock failure".into()));
        }
        if text.trim().is_empty() {
            return Err(GrammarError::EmptyText);
        }
        Ok(match &self.suffix {
            Some(s) => format!("{text}{s}"),
            None => text.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_appends_suffix() {
        let mut mock = MockGrammarPolisher::with_suffix("!");
        assert_eq!(mock.polish("hello").unwrap(), "hello!");
    }

    #[test]
    fn mock_rejects_empty() {
        let mut mock = MockGrammarPolisher::identity();
        assert!(matches!(mock.polish("  "), Err(GrammarError::EmptyText)));
    }

    #[test]
    fn mock_failure_surfaces_error() {
        let mut mock = MockGrammarPolisher::failing();
        assert!(mock.polish("hi").is_err());
    }
}
