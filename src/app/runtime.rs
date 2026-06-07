use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Receiver;
use tracing::{info, warn};

use crate::app::{AppEvent, UserAction};
use crate::app::single_instance::SingleInstance;
use crate::app::state::{AppState, StateMachine, TransitionError};
use crate::asr::{AsrError, AsrWorker};
use crate::audio::{AudioCapture, AudioError};
use crate::config::Config;
use crate::hotkeys;
use crate::output::{DeliveryChain, DeliveryError, DeliveryOutcome};
use crate::platform::win::process::foreground_process_name;
use crate::postprocess::{self, InputContext, PostProcessOptions};
use crate::ui::tray;

pub struct AppRuntime {
    state: StateMachine,
    config: Config,
    events: Receiver<AppEvent>,
    asr: AsrWorker,
    audio: Option<AudioCapture>,
    delivery: DeliveryChain,
    running: Arc<AtomicBool>,
    _single_instance: SingleInstance,
}

impl AppRuntime {
    pub fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let single_instance = SingleInstance::acquire()?;
        let config = Config::load();
        let asr = AsrWorker::spawn(config.directml);
        let delivery = DeliveryChain::new();

        let (event_tx, events) = crossbeam_channel::unbounded();
        let running = Arc::new(AtomicBool::new(true));

        eprintln!("Squeak initializing (tray + hotkeys)...");
        hotkeys::spawn_hotkeys(event_tx.clone());
        tray::spawn(event_tx.clone(), Arc::clone(&running))?;

        Ok(Self {
            state: StateMachine::new(),
            config,
            events,
            asr,
            audio: None,
            delivery,
            running,
            _single_instance: single_instance,
        })
    }

    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("Loading speech model in background (first launch may take a minute)...");
        self.asr.preload_in_background(self.config.model_tier);

        eprintln!("Hold Win+Ctrl to dictate. Shift+Alt+Z pastes last transcript. Tray menu → Exit to quit.");
        info!("Squeak running — hold Win+Ctrl to dictate, Shift+Alt+Z to paste last");

        while self.running.load(Ordering::Relaxed) {
            match self.events.recv_timeout(Duration::from_millis(100)) {
                Ok(event) => {
                    if let Err(err) = self.handle_event(event) {
                        warn!("event error: {err}");
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        self.running.store(false, Ordering::Relaxed);
        hotkeys::shutdown_hotkeys();
        Ok(())
    }

    fn handle_event(&mut self, event: AppEvent) -> Result<(), RuntimeError> {
        if matches!(event, AppEvent::UserAction(UserAction::Exit)) {
            self.running.store(false, Ordering::Relaxed);
            return Ok(());
        }

        if matches!(event, AppEvent::UserAction(UserAction::PasteLast)) {
            return self.handle_paste_last();
        }

        let prev = self.state.state();
        let next = self.state.apply(event).map_err(RuntimeError::State)?;

        match (prev, next) {
            (_, AppState::RecordingPtt | AppState::RecordingHandsFree) => {
                if prev == AppState::RecordingPtt || prev == AppState::RecordingHandsFree {
                    if let Some(audio) = self.audio.as_mut() {
                        let _ = audio.stop();
                    }
                }
                self.start_recording()?;
                info!("Recording ({next:?})");
            }
            (AppState::RecordingPtt | AppState::RecordingHandsFree, AppState::Processing) => {
                self.process_recording()?;
            }
            (AppState::RecordingPtt | AppState::RecordingHandsFree, AppState::Idle) => {
                if let Some(audio) = self.audio.as_mut() {
                    let _ = audio.stop();
                }
                info!("Recording cancelled");
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_paste_last(&self) -> Result<(), RuntimeError> {
        if matches!(
            self.state.state(),
            AppState::RecordingPtt | AppState::RecordingHandsFree
        ) {
            return Ok(());
        }
        match self.delivery.paste_last() {
            Ok(outcome) => info!("Paste-last: {outcome:?}"),
            Err(DeliveryError::NoLastTranscript) => info!("Paste-last: no prior transcript"),
            Err(e) => warn!("Paste-last failed: {e}"),
        }
        Ok(())
    }

    fn start_recording(&mut self) -> Result<(), RuntimeError> {
        if self.audio.is_none() {
            self.audio = Some(AudioCapture::try_new().map_err(RuntimeError::Audio)?);
        }
        self.audio
            .as_mut()
            .expect("audio just initialized")
            .start()
            .map_err(RuntimeError::Audio)
    }

    fn process_recording(&mut self) -> Result<(), RuntimeError> {
        let samples = self
            .audio
            .as_mut()
            .ok_or(RuntimeError::Message("microphone not available".into()))?
            .stop()
            .map_err(RuntimeError::Audio)?;
        info!("Processing {} samples", samples.len());

        if samples.is_empty() {
            return self.fail_processing("empty audio");
        }

        if self.asr.is_downloading() {
            return self.fail_processing("model is still downloading");
        }

        self.asr
            .ensure_ready(self.config.model_tier)
            .map_err(|e| RuntimeError::Message(e.to_string()))?;

        let raw = self.asr.transcribe(samples).map_err(|e| match e {
            AsrError::EmptyAudio => RuntimeError::Message("empty audio".into()),
            other => RuntimeError::Message(other.to_string()),
        })?;

        let context = foreground_process_name()
            .map(|name| postprocess::detect_context_from_process(&name))
            .unwrap_or(InputContext::Prose);
        let text = postprocess::postprocess(&raw, PostProcessOptions { context });

        if text.is_empty() {
            return self.fail_processing("empty transcript");
        }

        let target = DeliveryChain::choose_target();
        self.state
            .apply(AppEvent::TranscriptReady {
                text: text.clone(),
                target,
            })
            .map_err(RuntimeError::State)?;

        let outcome = self
            .delivery
            .deliver(&text, target)
            .map_err(RuntimeError::Delivery)?;

        let done = match outcome {
            DeliveryOutcome::Buffered => AppEvent::DeliveryBuffered,
            _ => AppEvent::DeliveryComplete,
        };
        self.state.apply(done).map_err(RuntimeError::State)?;
        info!("Transcript delivered ({outcome:?})");
        Ok(())
    }

    fn fail_processing(&mut self, message: &str) -> Result<(), RuntimeError> {
        warn!("Processing failed: {message}");
        self.state
            .apply(AppEvent::ProcessingFailed {
                message: message.into(),
            })
            .map_err(RuntimeError::State)?;
        self.state
            .apply(AppEvent::DismissError)
            .map_err(RuntimeError::State)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum RuntimeError {
    #[error(transparent)]
    State(#[from] TransitionError),

    #[error(transparent)]
    Audio(#[from] AudioError),

    #[error(transparent)]
    Delivery(#[from] DeliveryError),

    #[error("{0}")]
    Message(String),
}
