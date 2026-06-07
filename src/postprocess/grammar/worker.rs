use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use tracing::{info, warn};

use crate::config::GrammarModelId;
use crate::postprocess::grammar::engine::{GrammarError, GrammarPolisher};

#[cfg(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama"))]
use crate::postprocess::grammar::factory::create_polisher;
#[cfg(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama"))]
use crate::postprocess::grammar::provision::{ensure_model, DownloadProgress};

enum WorkerCommand {
    EnsureReady {
        model: GrammarModelId,
        reply: Sender<Result<(), GrammarError>>,
    },
    Polish {
        text: String,
        reply: Sender<Result<String, GrammarError>>,
    },
    Reload {
        model: GrammarModelId,
    },
    Shutdown,
}

/// Background grammar worker (mirrors `AsrWorker`).
pub struct GrammarWorker {
    tx: Sender<WorkerCommand>,
    downloading: Arc<AtomicBool>,
    _handle: JoinHandle<()>,
}

impl GrammarWorker {
    pub fn spawn() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let downloading = Arc::new(AtomicBool::new(false));
        let downloading_flag = Arc::clone(&downloading);

        let handle = thread::Builder::new()
            .name("squeak-grammar".into())
            .spawn(move || worker_main(rx, downloading_flag))
            .expect("failed to spawn grammar worker thread");

        Self {
            tx,
            downloading,
            _handle: handle,
        }
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading.load(Ordering::Relaxed)
    }

    pub fn preload_in_background(&self, model: GrammarModelId) {
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
            .name("squeak-grammar-preload".into())
            .spawn(move || {
                match reply_rx.recv() {
                    Ok(Ok(())) => eprintln!("Grammar model ready."),
                    Ok(Err(err)) => eprintln!("Grammar model load failed: {err}"),
                    Err(_) => eprintln!("Grammar model load interrupted."),
                }
            })
            .ok();
    }

    pub fn ensure_ready(&self, model: GrammarModelId) -> Result<(), GrammarError> {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        self.tx
            .send(WorkerCommand::EnsureReady {
                model,
                reply: reply_tx,
            })
            .map_err(|_| GrammarError::WorkerClosed)?;
        reply_rx.recv().map_err(|_| GrammarError::WorkerClosed)?
    }

    pub fn polish(&self, text: &str) -> Result<String, GrammarError> {
        if text.trim().is_empty() {
            return Err(GrammarError::EmptyText);
        }
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        self.tx
            .send(WorkerCommand::Polish {
                text: text.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| GrammarError::WorkerClosed)?;
        reply_rx.recv().map_err(|_| GrammarError::WorkerClosed)?
    }

    pub fn reload(&self, model: GrammarModelId) -> Result<(), GrammarError> {
        self.tx
            .send(WorkerCommand::Reload { model })
            .map_err(|_| GrammarError::WorkerClosed)
    }
}

impl Drop for GrammarWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerCommand::Shutdown);
    }
}

struct WorkerState {
    loaded: Option<GrammarModelId>,
    polisher: Option<Box<dyn GrammarPolisher>>,
}

fn worker_main(rx: Receiver<WorkerCommand>, downloading: Arc<AtomicBool>) {
    let mut state = WorkerState {
        loaded: None,
        polisher: None,
    };

    for cmd in rx {
        match cmd {
            WorkerCommand::EnsureReady { model, reply } => {
                let result = ensure_ready(&mut state, model, &downloading);
                let _ = reply.send(result);
            }
            WorkerCommand::Polish { text, reply } => {
                let result = polish_loaded(&mut state, &text, &downloading);
                let _ = reply.send(result);
            }
            WorkerCommand::Reload { model } => {
                state.polisher = None;
                state.loaded = Some(model);
                info!(
                    "Grammar model scheduled for reload on next ensure_ready ({})",
                    model.config_key()
                );
            }
            WorkerCommand::Shutdown => break,
        }
    }

    info!("Grammar worker shutting down");
}

fn ensure_ready(
    state: &mut WorkerState,
    model: GrammarModelId,
    downloading: &AtomicBool,
) -> Result<(), GrammarError> {
    if state.loaded == Some(model) && state.polisher.is_some() {
        return Ok(());
    }

    #[cfg(not(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama")))]
    {
        let _ = (state, model, downloading);
        return Err(GrammarError::Unavailable);
    }

    #[cfg(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama"))]
    {
        downloading.store(true, Ordering::Relaxed);
        let download_result = ensure_model(model, |progress| match &progress {
            DownloadProgress::Starting { model } => {
                info!("Downloading grammar model: {}", model.config_key())
            }
            DownloadProgress::Downloading {
                downloaded,
                total,
            } => info!("Grammar download: {downloaded} / {:?}", total),
            DownloadProgress::Extracting => info!("Extracting grammar model files"),
            DownloadProgress::Complete => info!("Grammar model download complete"),
        });
        downloading.store(false, Ordering::Relaxed);

        let dir = download_result?;
        state.polisher = Some(create_polisher(model, &dir)?);
        state.loaded = Some(model);
        info!("Grammar model warm-loaded: {}", model.config_key());
        Ok(())
    }
}

fn polish_loaded(
    state: &mut WorkerState,
    text: &str,
    downloading: &AtomicBool,
) -> Result<String, GrammarError> {
    if downloading.load(Ordering::Relaxed) {
        return Err(GrammarError::Downloading);
    }

    let polisher = state.polisher.as_mut().ok_or(GrammarError::NotLoaded)?;
    polisher.polish(text)
}

/// Fall back to rules-only text when polish fails (R12).
pub fn polish_or_fallback(worker: &GrammarWorker, text: &str, model: GrammarModelId) -> String {
    if text.trim().is_empty() {
        return text.to_string();
    }

    match worker.ensure_ready(model) {
        Ok(()) => match worker.polish(text) {
            Ok(polished) => polished,
            Err(err) => {
                warn!("Grammar polish failed, using rules-only text: {err}");
                text.to_string()
            }
        },
        Err(err) => {
            warn!("Grammar model not ready, using rules-only text: {err}");
            text.to_string()
        }
    }
}
