use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use tracing::info;

use crate::asr::engine::{AsrEngine, AsrError};
use crate::asr::factory::create_engine;
use crate::asr::provision::{ensure_model, DownloadProgress};
use crate::asr::moonshine::configure_ort_accelerator;
use crate::config::AsrModelId;

enum WorkerCommand {
    IsReady {
        model: AsrModelId,
        reply: Sender<bool>,
    },
    EnsureReady {
        model: AsrModelId,
        reply: Sender<Result<(), AsrError>>,
    },
    Transcribe {
        samples: Vec<f32>,
        reply: Sender<Result<String, AsrError>>,
    },
    Reload {
        model: AsrModelId,
    },
    Shutdown,
}

/// Handle to the background ASR worker (owns loaded engine on a dedicated thread).
pub struct AsrWorker {
    tx: Sender<WorkerCommand>,
    downloading: Arc<AtomicBool>,
    _handle: JoinHandle<()>,
}

impl AsrWorker {
    pub fn spawn(use_directml: bool) -> Self {
        configure_ort_accelerator(use_directml);
        let (tx, rx) = crossbeam_channel::unbounded();
        let downloading = Arc::new(AtomicBool::new(false));
        let downloading_flag = Arc::clone(&downloading);

        let handle = thread::Builder::new()
            .name("squeak-asr".into())
            .spawn(move || worker_main(rx, downloading_flag))
            .expect("failed to spawn ASR worker thread");

        Self {
            tx,
            downloading,
            _handle: handle,
        }
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading.load(Ordering::Relaxed)
    }

    pub fn preload_in_background(&self, model: AsrModelId) {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        if self
            .tx
            .send(WorkerCommand::EnsureReady {
                model,
                reply: reply_tx,
            })
            .is_err()
        {
            return;
        }
        thread::Builder::new()
            .name("squeak-asr-preload".into())
            .spawn(move || {
                match reply_rx.recv() {
                    Ok(Ok(())) => eprintln!("Speech model ready."),
                    Ok(Err(err)) => eprintln!("Speech model load failed: {err}"),
                    Err(_) => eprintln!("Speech model load interrupted."),
                }
            })
            .ok();
    }

    pub fn is_ready(&self, model: AsrModelId) -> bool {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        if self
            .tx
            .send(WorkerCommand::IsReady {
                model,
                reply: reply_tx,
            })
            .is_err()
        {
            return false;
        }
        reply_rx.recv().unwrap_or(false)
    }

    pub fn ensure_ready(&self, model: AsrModelId) -> Result<(), AsrError> {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        self.tx
            .send(WorkerCommand::EnsureReady {
                model,
                reply: reply_tx,
            })
            .map_err(|_| AsrError::WorkerClosed)?;
        reply_rx.recv().map_err(|_| AsrError::WorkerClosed)?
    }

    pub fn transcribe(&self, samples: Vec<f32>) -> Result<String, AsrError> {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        self.tx
            .send(WorkerCommand::Transcribe {
                samples,
                reply: reply_tx,
            })
            .map_err(|_| AsrError::WorkerClosed)?;
        reply_rx.recv().map_err(|_| AsrError::WorkerClosed)?
    }

    pub fn reload(&self, model: AsrModelId) -> Result<(), AsrError> {
        self.tx
            .send(WorkerCommand::Reload { model })
            .map_err(|_| AsrError::WorkerClosed)
    }
}

impl Drop for AsrWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerCommand::Shutdown);
    }
}

struct WorkerState {
    loaded: Option<AsrModelId>,
    engine: Option<Box<dyn AsrEngine>>,
}

fn worker_main(rx: Receiver<WorkerCommand>, downloading: Arc<AtomicBool>) {
    let mut state = WorkerState {
        loaded: None,
        engine: None,
    };

    for cmd in rx {
        match cmd {
            WorkerCommand::IsReady { model, reply } => {
                let ready = state.loaded == Some(model) && state.engine.is_some();
                let _ = reply.send(ready);
            }
            WorkerCommand::EnsureReady { model, reply } => {
                let result = ensure_ready(&mut state, model, &downloading);
                let _ = reply.send(result);
            }
            WorkerCommand::Transcribe { samples, reply } => {
                let result = transcribe_loaded(&mut state, &samples, &downloading);
                let _ = reply.send(result);
            }
            WorkerCommand::Reload { model } => {
                state.engine = None;
                state.loaded = Some(model);
                info!(
                    "ASR model scheduled for reload on next ensure_ready ({})",
                    model.config_key()
                );
            }
            WorkerCommand::Shutdown => break,
        }
    }

    info!("ASR worker shutting down");
}

fn ensure_ready(
    state: &mut WorkerState,
    model: AsrModelId,
    downloading: &AtomicBool,
) -> Result<(), AsrError> {
    if state.loaded == Some(model) && state.engine.is_some() {
        return Ok(());
    }

    downloading.store(true, Ordering::Relaxed);
    let download_result = ensure_model(model, |progress| match &progress {
        DownloadProgress::Starting { model } => {
            info!("Downloading speech model: {}", model.config_key())
        }
        DownloadProgress::Downloading {
            downloaded,
            total,
        } => info!("Model download: {downloaded} / {:?}", total),
        DownloadProgress::Extracting => info!("Extracting model archive"),
        DownloadProgress::Complete => info!("Model download complete"),
    });
    downloading.store(false, Ordering::Relaxed);

    let dir = download_result?;

    state.engine = Some(create_engine(model, &dir)?);
    state.loaded = Some(model);
    info!("Speech model warm-loaded: {}", model.config_key());
    Ok(())
}

fn transcribe_loaded(
    state: &mut WorkerState,
    samples: &[f32],
    downloading: &AtomicBool,
) -> Result<String, AsrError> {
    if downloading.load(Ordering::Relaxed) {
        return Err(AsrError::Downloading);
    }

    let engine = state.engine.as_mut().ok_or(AsrError::NotLoaded)?;
    engine.transcribe(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::engine::MockAsrEngine;

    #[test]
    fn mock_engine_still_works_in_worker_tests() {
        let mut mock = MockAsrEngine::new("ok");
        assert_eq!(mock.transcribe(&[0.5]).unwrap(), "ok");
    }
}
