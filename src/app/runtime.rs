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
    log_audio_stats, maybe_write_debug_wav, peak_normalize, AudioCapture, AudioError, AudioLevelMeter,
};
use crate::config::{AsrModelId, Config, GrammarModelId};
use crate::hotkeys;
use crate::output::{DeliveryChain, DeliveryError};
use crate::platform::win::focus::FocusTarget;
use crate::platform::win::process::foreground_process_name;
use crate::postprocess::{self, GrammarWorker, InputContext, PostProcessOptions};
use crate::ui::{overlay, tray};

pub struct AppRuntime {
    state: StateMachine,
    config: Config,
    events: Receiver<AppEvent>,
    asr: AsrWorker,
    grammar: GrammarWorker,
    audio: Option<AudioCapture>,
    delivery: DeliveryChain,
    running: Arc<AtomicBool>,
    overlay_tx: crossbeam_channel::Sender<overlay::OverlayMode>,
    audio_meter: Arc<AudioLevelMeter>,
    injection_target: Option<FocusTarget>,
    _single_instance: SingleInstance,
}

impl AppRuntime {
    pub fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let single_instance = SingleInstance::acquire()?;
        let config = Config::load();
        let asr = AsrWorker::spawn(config.directml);
        let grammar = GrammarWorker::spawn();
        let delivery = DeliveryChain::new();

        let (event_tx, events) = crossbeam_channel::unbounded();
        let (overlay_tx, overlay_rx) = crossbeam_channel::unbounded();
        let audio_meter = AudioLevelMeter::new();
        let running = Arc::new(AtomicBool::new(true));

        eprintln!("Squeak initializing (tray + hotkeys + overlay)...");
        hotkeys::spawn_hotkeys(event_tx.clone());
        tray::spawn(
            event_tx.clone(),
            Arc::clone(&running),
            config.asr_model(),
            config.grammar_enabled(),
            config.grammar_model(),
        )?;
        overlay::spawn(overlay_rx, Arc::clone(&running), Arc::clone(&audio_meter))?;

        Ok(Self {
            state: StateMachine::new(),
            config,
            events,
            asr,
            grammar,
            audio: None,
            delivery,
            running,
            overlay_tx,
            audio_meter,
            injection_target: None,
            _single_instance: single_instance,
        })
    }

    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("Loading speech model in background (first launch may take a minute)...");
        self.asr.preload_in_background(self.config.asr_model());
        if self.config.grammar_enabled() {
            self.grammar
                .preload_in_background(self.config.grammar_model());
        }

        eprintln!("Hold Win+Ctrl to dictate. Shift+Alt+Z pastes last transcript. Orange circle in the taskbar = Squeak running.");
        eprintln!(
            "Using {} — tray → Speech model to change (Small recommended).",
            self.config.asr_model().tray_summary()
        );
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

        if let AppEvent::UserAction(UserAction::SetAsrModel(key)) = &event {
            return self.handle_set_asr_model(key);
        }

        if let AppEvent::UserAction(UserAction::SetModelTier(tier_name)) = &event {
            return self.handle_set_asr_model(tier_name);
        }

        if let AppEvent::UserAction(UserAction::SetGrammarProfile(key)) = &event {
            return self.handle_set_grammar_profile(key);
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

    fn handle_set_asr_model(&mut self, key: &str) -> Result<(), RuntimeError> {
        let model = AsrModelId::parse(key).ok_or_else(|| {
            RuntimeError::Message(format!("unknown speech model: {key}"))
        })?;

        if self.config.asr_model() == model {
            info!("Speech model already {}", model.config_key());
            return Ok(());
        }

        self.config.set_asr_model(model);
        self.config.save().map_err(|e| RuntimeError::Message(e.to_string()))?;
        self.asr
            .reload(model)
            .map_err(|e| RuntimeError::Message(e.to_string()))?;
        self.asr.preload_in_background(model);
        eprintln!(
            "Speech model set to {}. Download/load may take a minute on first use.",
            model.tray_summary()
        );
        info!("Speech model changed to {}", model.config_key());
        Ok(())
    }

    fn handle_set_grammar_profile(&mut self, key: &str) -> Result<(), RuntimeError> {
        let key = key.trim().to_lowercase();
        if key == "off" {
            if !self.config.grammar_enabled() {
                return Ok(());
            }
            self.config.set_grammar_enabled(false);
            self.config.save().map_err(|e| RuntimeError::Message(e.to_string()))?;
            eprintln!("Grammar correction disabled.");
            info!("Grammar correction disabled");
            return Ok(());
        }

        let model = GrammarModelId::parse(&key).ok_or_else(|| {
            RuntimeError::Message(format!("unknown grammar profile: {key}"))
        })?;

        let unchanged = self.config.grammar_enabled() && self.config.grammar_model() == model;
        if unchanged {
            info!("Grammar profile already {}", model.config_key());
            return Ok(());
        }

        self.config.set_grammar_enabled(true);
        self.config.set_grammar_model(model);
        self.config.save().map_err(|e| RuntimeError::Message(e.to_string()))?;
        self.grammar
            .reload(model)
            .map_err(|e| RuntimeError::Message(e.to_string()))?;
        self.grammar.preload_in_background(model);
        eprintln!(
            "Grammar correction set to {}. Download/load may take a minute on first use.",
            model.tray_summary()
        );
        info!("Grammar profile changed to {}", model.config_key());
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
            self.audio = Some(
                AudioCapture::try_new_with_meter(Some(Arc::clone(&self.audio_meter)))
                    .map_err(RuntimeError::Audio)?,
            );
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
            .ensure_ready(self.config.asr_model())
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
        let options = PostProcessOptions {
            context,
            grammar_enabled: self.config.grammar_enabled(),
        };
        let text = postprocess::postprocess_with_worker(
            &raw,
            options,
            Some(&self.grammar),
            self.config.grammar_model(),
        );

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
