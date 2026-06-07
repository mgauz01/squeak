use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Receiver;
use tracing::{info, warn};

use crate::app::{AppEvent, UserAction};
use crate::app::single_instance::SingleInstance;
use crate::app::state::{AppState, StateMachine, TransitionError};
use crate::asr::{AsrError, AsrWorker};
use crate::audio::{
    log_audio_stats, maybe_write_debug_wav, peak_normalize, AudioCapture, AudioError,
};
use crate::config::{Config, ModelTier};
use crate::hotkeys;
use crate::output::{DeliveryChain, DeliveryError};
use crate::platform::win::focus::FocusTarget;
use crate::platform::win::process::foreground_process_name;
use crate::postprocess::{self, InputContext, PostProcessOptions};
use crate::ui::{overlay, tray};

pub struct AppRuntime {
    state: StateMachine,
    config: Config,
    events: Receiver<AppEvent>,
    asr: AsrWorker,
    audio: Option<AudioCapture>,
    delivery: DeliveryChain,
    running: Arc<AtomicBool>,
    overlay_tx: crossbeam_channel::Sender<overlay::OverlayMode>,
    injection_target: Option<FocusTarget>,
    _single_instance: SingleInstance,
}

impl AppRuntime {
    pub fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let single_instance = SingleInstance::acquire()?;
        let config = Config::load();
        let asr = AsrWorker::spawn(config.directml);
        let delivery = DeliveryChain::new();

        let (event_tx, events) = crossbeam_channel::unbounded();
        let (overlay_tx, overlay_rx) = crossbeam_channel::unbounded();
        let running = Arc::new(AtomicBool::new(true));

        eprintln!("Squeak initializing (tray + hotkeys + overlay)...");
        hotkeys::spawn_hotkeys(event_tx.clone());
        tray::spawn(event_tx.clone(), Arc::clone(&running), config.model_tier)?;
        overlay::spawn(overlay_rx, Arc::clone(&running))?;

        Ok(Self {
            state: StateMachine::new(),
            config,
            events,
            asr,
            audio: None,
            delivery,
            running,
            overlay_tx,
            injection_target: None,
            _single_instance: single_instance,
        })
    }

    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("Loading speech model in background (first launch may take a minute)...");
        self.asr.preload_in_background(self.config.model_tier);

        eprintln!("Hold Win+Ctrl to dictate. Shift+Alt+Z pastes last transcript. Orange circle in the taskbar = Squeak running.");
        eprintln!("Using {:?} speech model — tray → Speech model to change tier (Small recommended).", self.config.model_tier);
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
            overlay::sync(&self.overlay_tx, self.state.state());
            return Ok(());
        }

        if matches!(event, AppEvent::UserAction(UserAction::PasteLast)) {
            return self.handle_paste_last();
        }

        if let AppEvent::UserAction(UserAction::SetModelTier(tier_name)) = &event {
            return self.handle_set_model_tier(tier_name);
        }

        let prev = self.state.state();
        let next = self.state.apply(event).map_err(RuntimeError::State)?;
        overlay::sync(&self.overlay_tx, next);

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
                self.injection_target = None;
                info!("Recording cancelled");
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_set_model_tier(&mut self, tier_name: &str) -> Result<(), RuntimeError> {
        let tier = ModelTier::parse(tier_name).ok_or_else(|| {
            RuntimeError::Message(format!("unknown model tier: {tier_name}"))
        })?;

        if self.config.model_tier == tier {
            info!("Model tier already {tier:?}");
            return Ok(());
        }

        self.config.model_tier = tier;
        self.config.save().map_err(|e| RuntimeError::Message(e.to_string()))?;
        self.asr
            .reload(tier)
            .map_err(|e| RuntimeError::Message(e.to_string()))?;
        self.asr.preload_in_background(tier);
        eprintln!(
            "Speech model set to {tier:?}. Download/load may take a minute on first use."
        );
        info!("Model tier changed to {tier:?}");
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
        self.injection_target = FocusTarget::capture();
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
        let mut samples = self
            .audio
            .as_mut()
            .ok_or(RuntimeError::Message("microphone not available".into()))?
            .stop()
            .map_err(RuntimeError::Audio)?;

        let stats = peak_normalize(&mut samples);
        log_audio_stats(stats);
        maybe_write_debug_wav(&samples);

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
            AsrError::AudioTooShort { samples, min } => RuntimeError::Message(format!(
                "recording too short ({samples} samples; hold Win+Ctrl longer and speak before release — need at least {min})"
            )),
            other => RuntimeError::Message(other.to_string()),
        })?;

        let context = foreground_process_name()
            .map(|name| postprocess::detect_context_from_process(&name))
            .unwrap_or(InputContext::Prose);
        let text = postprocess::postprocess(&raw, PostProcessOptions { context });

        if text.is_empty() {
            return self.fail_processing("empty transcript");
        }

        info!("Transcript: {text:?}");

        self.state
            .apply(AppEvent::TranscriptReady {
                text: text.clone(),
                target: crate::app::DeliveryTarget::InjectAtCaret,
            })
            .map_err(RuntimeError::State)?;

        let captured = self.injection_target.take();
        let outcome = self
            .delivery
            .deliver(&text, captured)
            .map_err(RuntimeError::Delivery)?;

        self.state
            .apply(AppEvent::DeliveryComplete)
            .map_err(RuntimeError::State)?;
        overlay::sync(&self.overlay_tx, self.state.state());
        info!("Delivery finished ({outcome:?})");
        Ok(())
    }

    fn fail_processing(&mut self, message: &str) -> Result<(), RuntimeError> {
        self.injection_target = None;
        eprintln!("Dictation failed: {message}");
        warn!("Processing failed: {message}");
        self.state
            .apply(AppEvent::ProcessingFailed {
                message: message.into(),
            })
            .map_err(RuntimeError::State)?;
        self.state
            .apply(AppEvent::DismissError)
            .map_err(RuntimeError::State)?;
        overlay::sync(&self.overlay_tx, self.state.state());
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
