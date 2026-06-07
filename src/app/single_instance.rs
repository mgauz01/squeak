use thiserror::Error;

#[derive(Debug, Error)]
pub enum SingleInstanceError {
    #[error("another Squeak instance is already running")]
    AlreadyRunning,

    #[cfg(windows)]
    #[error("failed to create single-instance mutex: {0}")]
    Win32(String),
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use super::SingleInstanceError;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};

    const MUTEX_NAME: &str = "Global\\Squeak.SingleInstance";

    pub struct SingleInstance {
        handle: HANDLE,
    }

    impl SingleInstance {
        pub fn acquire() -> Result<Self, SingleInstanceError> {
            let wide: Vec<u16> = OsStr::new(MUTEX_NAME).encode_wide().chain([0]).collect();
            unsafe {
                let handle = CreateMutexW(None, true, PCWSTR(wide.as_ptr()))
                    .map_err(|e| SingleInstanceError::Win32(e.to_string()))?;

                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = CloseHandle(handle);
                    return Err(SingleInstanceError::AlreadyRunning);
                }

                Ok(Self { handle })
            }
        }
    }

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            unsafe {
                let _ = ReleaseMutex(self.handle);
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(windows)]
pub use windows_impl::SingleInstance;

#[cfg(not(windows))]
pub struct SingleInstance;

#[cfg(not(windows))]
impl SingleInstance {
    pub fn acquire() -> Result<Self, SingleInstanceError> {
        Ok(Self)
    }
}
