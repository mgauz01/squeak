use thiserror::Error;

#[derive(Debug, Error)]
pub enum SingleInstanceError {
    #[error("another Squeak instance is already running")]
    AlreadyRunning,
}

/// Named mutex single-instance guard (Win32 impl in later unit).
pub struct SingleInstance;

impl SingleInstance {
    pub fn acquire() -> Result<Self, SingleInstanceError> {
        #[cfg(windows)]
        {
            let _ = Self;
            // TODO(U3): CreateMutexW Global\\Squeak.SingleInstance
            Ok(Self)
        }
        #[cfg(not(windows))]
        {
            Ok(Self)
        }
    }
}
