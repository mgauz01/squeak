use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use tracing::{info, warn};

use crate::app::AppEvent;
use crate::asr::engine::{AsrEngine, AsrError};
use crate::asr::factory::create_engine;
use crate::asr::engine::recommended_thread_count;
use crate::asr::moonshine::{configure_ort_runtime, is_likely_directml_inference_error};
use crate::asr::provision::{ensure_model, DownloadProgress};
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
    ApplyOrtConfig {
        ort: AsrWorkerConfig,
        model: AsrModelId,
    },
    Shutdown,
}

/// CPU/ORT settings for the background ASR worker.
#[derive(Debug, Clone, Copy)]
pub struct AsrWorkerConfig {
    pub directml: bool,
    pub threads: usize,
    pub xnnpack: bool,
}

impl AsrWorkerConfig {
    pub fn from_app_config(config: &crate::config::Config) -> Self {
        Self {
            directml: config.directml,
            threads: config.asr_thread_count(),
            xnnpack: config.xnnpack,
        }
    }
}

impl Default for AsrWorkerConfig {
    fn default() -> Self {
        Self {
            directml: false,
            threads: recommended_thread_count(),
            xnnpack: false,
        }
    }
}

/// Handle to the background ASR worker (owns loaded engine on a dedicated thread).
pub struct AsrWorker {
    tx: Sender<WorkerCommand>,
    downloading: Arc<AtomicBool>,
    _handle: JoinHandle<()>,
}

impl AsrWorker {
    pub fn spawn(config: AsrWorkerConfig) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let downloading = Arc::new(AtomicBool::new(false));
        let downloading_flag = Arc::clone(&downloading);

        let handle = thread::Builder::new()
            .name("squeak-asr".into())
            .spawn(move || worker_main(rx, downloading_flag, config))
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

    pub fn preload_in_background(&self, model: AsrModelId, notify: Option<Sender<AppEvent>>) {
        if self.is_ready(model) {
            if let Some(tx) = notify {
                let _ = tx.send(AppEvent::AsrModelReady);
            }
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
            .spawn(move || match reply_rx.recv() {
                Ok(Ok(())) => {
                    eprintln!("Speech model ready.");
                    if let Some(tx) = notify {
                        let _ = tx.send(AppEvent::AsrModelReady);
                    }
                }
                Ok(Err(err)) => eprintln!("Speech model load failed: {err}"),
                Err(_) => eprintln!("Speech model load interrupted."),
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

    /// Apply CPU/GPU runtime settings and schedule a model reload on the worker thread.
    pub fn apply_ort_config(&self, ort: AsrWorkerConfig, model: AsrModelId) -> Result<(), AsrError> {
        self.tx
            .send(WorkerCommand::ApplyOrtConfig { ort, model })
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
    ort: AsrWorkerConfig,
    /// After a DirectML inference failure, stay on CPU for the rest of this session.
    force_cpu: bool,
    loaded_on_directml: bool,
}

fn worker_main(rx: Receiver<WorkerCommand>, downloading: Arc<AtomicBool>, ort: AsrWorkerConfig) {
    let mut state = WorkerState {
        loaded: None,
        engine: None,
        ort,
        force_cpu: false,
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
            WorkerCommand::ApplyOrtConfig { ort, model } => {
                state.ort = ort;
                state.engine = None;
                state.loaded = Some(model);
                state.force_cpu = false;
                info!(
                    "ASR runtime config updated ({} threads); model will reload",
                    ort.threads
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
        DownloadProgress::Downloading { downloaded, total } => {
            info!("Model download: {downloaded} / {:?}", total)
        }
        DownloadProgress::Extracting => info!("Extracting model archive"),
        DownloadProgress::Complete => info!("Model download complete"),
    });
    downloading.store(false, Ordering::Relaxed);

    let dir = download_result?;

    load_engine(state, model, &dir)?;
    Ok(())
}

fn use_directml(state: &WorkerState, model: AsrModelId) -> bool {
    state.ort.directml
        && !state.force_cpu
        && model.compatible_with_directml()
        && cfg!(feature = "directml")
}

fn load_engine(
    state: &mut WorkerState,
    model: AsrModelId,
    dir: &std::path::Path,
) -> Result<(), AsrError> {
    let on_directml = use_directml(state, model);
    configure_ort_runtime(
        model,
        on_directml,
        state.ort.threads,
        state.ort.xnnpack && !on_directml,
    );
    state.loaded_on_directml = on_directml;
    state.engine = Some(create_engine(model, dir)?);
    state.loaded = Some(model);
    warmup_engine(state);
    info!("Speech model warm-loaded: {}", model.config_key());
    Ok(())
}

/// Prime ONNX graphs so the first real dictation is not paying cold-start cost.
fn warmup_engine(state: &mut WorkerState) {
    const WARMUP_SAMPLES: usize = 16_000;
    let samples = vec![0.0f32; WARMUP_SAMPLES];
    let Some(engine) = state.engine.as_mut() else {
        return;
    };
    match engine.transcribe(&samples) {
        Ok(_) => info!("ASR warmup complete"),
        Err(AsrError::EmptyAudio) | Err(AsrError::AudioTooShort { .. }) => {
            info!("ASR warmup complete (short/silent clip)");
        }
        Err(err) => warn!("ASR warmup failed (non-fatal): {err}"),
    }
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
    state.force_cpu = true;
    state.loaded_on_directml = false;
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

    #[test]
    fn force_cpu_disables_directml_after_fallback() {
        let state = WorkerState {
            loaded: None,
            engine: None,
            ort: AsrWorkerConfig {
                directml: true,
                threads: 4,
                xnnpack: false,
            },
            force_cpu: true,
            loaded_on_directml: false,
        };
        assert!(!use_directml(
            &state,
            AsrModelId::moonshine(crate::config::ModelTier::Small)
        ));
    }
}
