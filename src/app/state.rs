use super::events::{AppEvent, RecordingMode};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Idle,
    RecordingPtt,
    RecordingHandsFree,
    Processing,
    Injecting,
    Buffered,
    Error,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransitionError {
    #[error("invalid transition from {from:?} on {event:?}")]
    Invalid { from: AppState, event: AppEvent },
}

pub struct StateMachine {
    state: AppState,
}

impl Default for StateMachine {
    fn default() -> Self {
        Self {
            state: AppState::Idle,
        }
    }
}

/// States where an in-app MSI update must not start.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_update_blocked_state(state: AppState) -> bool {
    matches!(
        state,
        AppState::RecordingPtt
            | AppState::RecordingHandsFree
            | AppState::Processing
            | AppState::Injecting
    )
}

impl StateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> AppState {
        self.state
    }

    pub fn apply(&mut self, event: AppEvent) -> Result<AppState, TransitionError> {
        let next = transition(self.state, &event).ok_or(TransitionError::Invalid {
            from: self.state,
            event,
        })?;
        self.state = next;
        Ok(next)
    }
}

fn transition(from: AppState, event: &AppEvent) -> Option<AppState> {
    use AppEvent::*;
    use AppState::*;

    match (from, event) {
        (
            Idle,
            StartRecording {
                mode: RecordingMode::PushToTalk,
            },
        ) => Some(RecordingPtt),
        (
            Idle,
            StartRecording {
                mode: RecordingMode::HandsFree,
            },
        ) => Some(RecordingHandsFree),
        (
            Buffered,
            StartRecording {
                mode: RecordingMode::PushToTalk,
            },
        ) => Some(RecordingPtt),
        (
            Buffered,
            StartRecording {
                mode: RecordingMode::HandsFree,
            },
        ) => Some(RecordingHandsFree),

        (RecordingPtt, StopRecording) | (RecordingHandsFree, StopRecording) => Some(Processing),

        (RecordingPtt, CancelRecording)
        | (RecordingHandsFree, CancelRecording)
        | (RecordingPtt, StartRecording { .. })
        | (RecordingHandsFree, StartRecording { .. }) => Some(Idle),

        (Processing, TranscriptReady { .. }) => Some(Injecting),
        (Processing, ProcessingFailed { .. }) => Some(Error),

        (Injecting, DeliveryComplete) => Some(Idle),
        (Injecting, DeliveryBuffered) => Some(Buffered),
        (
            Injecting,
            StartRecording {
                mode: RecordingMode::PushToTalk,
            },
        ) => Some(RecordingPtt),
        (
            Injecting,
            StartRecording {
                mode: RecordingMode::HandsFree,
            },
        ) => Some(RecordingHandsFree),

        (Buffered, DeliveryComplete) | (Buffered, DismissError) => Some(Idle),

        (Error, DismissError) => Some(Idle),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::events::DeliveryTarget;

    #[test]
    fn ptt_happy_path() {
        let mut sm = StateMachine::new();
        assert_eq!(sm.state(), AppState::Idle);

        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        })
        .unwrap();
        assert_eq!(sm.state(), AppState::RecordingPtt);

        sm.apply(AppEvent::StopRecording).unwrap();
        assert_eq!(sm.state(), AppState::Processing);

        sm.apply(AppEvent::TranscriptReady {
            text: "hello".into(),
            target: DeliveryTarget::InjectAtCaret,
        })
        .unwrap();
        assert_eq!(sm.state(), AppState::Injecting);

        sm.apply(AppEvent::DeliveryComplete).unwrap();
        assert_eq!(sm.state(), AppState::Idle);
    }

    #[test]
    fn mode_switch_cancels_recording() {
        let mut sm = StateMachine::new();
        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::HandsFree,
        })
        .unwrap();
        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        })
        .unwrap();
        assert_eq!(sm.state(), AppState::Idle);
    }

    #[test]
    fn processing_failure_returns_to_idle_via_error() {
        let mut sm = StateMachine::new();
        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        })
        .unwrap();
        sm.apply(AppEvent::StopRecording).unwrap();
        sm.apply(AppEvent::ProcessingFailed {
            message: "asr failed".into(),
        })
        .unwrap();
        assert_eq!(sm.state(), AppState::Error);
        sm.apply(AppEvent::DismissError).unwrap();
        assert_eq!(sm.state(), AppState::Idle);
    }

    #[test]
    fn buffered_allows_new_recording() {
        let mut sm = StateMachine::new();
        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        })
        .unwrap();
        sm.apply(AppEvent::StopRecording).unwrap();
        sm.apply(AppEvent::TranscriptReady {
            text: "hello".into(),
            target: DeliveryTarget::InjectAtCaret,
        })
        .unwrap();
        sm.apply(AppEvent::DeliveryBuffered).unwrap();
        assert_eq!(sm.state(), AppState::Buffered);
        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        })
        .unwrap();
        assert_eq!(sm.state(), AppState::RecordingPtt);
    }

    #[test]
    fn update_blocked_while_recording_or_processing() {
        assert!(is_update_blocked_state(AppState::RecordingPtt));
        assert!(is_update_blocked_state(AppState::Processing));
        assert!(is_update_blocked_state(AppState::Injecting));
        assert!(!is_update_blocked_state(AppState::Idle));
    }

    #[test]
    fn injecting_recovers_on_new_recording() {
        let mut sm = StateMachine::new();
        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        })
        .unwrap();
        sm.apply(AppEvent::StopRecording).unwrap();
        sm.apply(AppEvent::TranscriptReady {
            text: "hello".into(),
            target: DeliveryTarget::InjectAtCaret,
        })
        .unwrap();
        assert_eq!(sm.state(), AppState::Injecting);

        sm.apply(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        })
        .unwrap();
        assert_eq!(sm.state(), AppState::RecordingPtt);
    }

    #[test]
    fn invalid_transition_rejected() {
        let mut sm = StateMachine::new();
        assert!(sm.apply(AppEvent::StopRecording).is_err());
    }
}
