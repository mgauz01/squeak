use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use tracing::{info, warn};

use crate::asr::engine::{AsrEngine, AsrError};
use crate::asr::factory::create_engine;
use crate::asr::provision::{ensure_model, DownloadProgress};
use crate::asr::moonshine::{configure_ort_accelerator_for_model, is_likely_directml_inference_error};
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
        let (tx, rx) = crossbeam_channel::unbounded();
        let downloading = Arc::new(AtomicBool::new(false));
        let downloading_flag = Arc::clone(&downloading);

        let handle = thread::Builder::new()
            .name("squeak-asr".into())
            .spawn(move || worker_main(rx, downloading_flag, use_directml))
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
        if self.is_ready(model) {
            return;
        }
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
    prefer_directml: bool,
    loaded_on_directml: bool,
}

fn worker_main(rx: Receiver<WorkerCommand>, downloading: Arc<AtomicBool>, prefer_directml: bool) {
    let mut state = WorkerState {
        loaded: None,
        engine: None,
        prefer_directml,
        loaded_on_directml: false,
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

    load_engine(state, model, &dir)?;
    Ok(())
}

fn load_engine(
    state: &mut WorkerState,
    model: AsrModelId,
    dir: &std::path::Path,
) -> Result<(), AsrError> {
    configure_ort_accelerator_for_model(model, state.prefer_directml);
    state.loaded_on_directml =
        state.prefer_directml && model.compatible_with_directml() && cfg!(feature = "directml");
    state.engine = Some(create_engine(model, dir)?);
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

    let model = state.loaded.ok_or(AsrError::NotLoaded)?;
    let engine = state.engine.as_mut().ok_or(AsrError::NotLoaded)?;
    match engine.transcribe(samples) {
        Ok(text) => Ok(text),
        Err(err) if state.loaded_on_directml && is_transient_directml_error(&err) => {
            warn!("DirectML transcription failed ({err}); reloading model on CPU");
            retry_transcribe_on_cpu(state, model, samples)
        }
        Err(err) => Err(err),
    }
}

fn is_transient_directml_error(err: &AsrError) -> bool {
    match err {
        AsrError::Transcription(message) | AsrError::Other(message) => {
            is_likely_directml_inference_error(message)
        }
        _ => false,
    }
}

fn retry_transcribe_on_cpu(
    state: &mut WorkerState,
    model: AsrModelId,
    samples: &[f32],
) -> Result<String, AsrError> {
    state.engine = None;
    state.loaded_on_directml = false;
    configure_ort_accelerator_for_model(model, false);
    let dir = crate::asr::provision::model_dir(model);
    load_engine(state, model, &dir)?;
    eprintln!("Speech model reloaded on CPU after DirectML failure.");
    state
        .engine
        .as_mut()
        .ok_or(AsrError::NotLoaded)?
        .transcribe(samples)
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
