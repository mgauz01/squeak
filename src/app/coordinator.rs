use crossbeam_channel::{Receiver, Sender};

use super::events::AppEvent;
use super::state::{AppState, StateMachine, TransitionError};

/// Central coordinator skeleton — wires channels and state transitions (U3).
pub struct Coordinator {
    state: StateMachine,
    event_tx: Sender<AppEvent>,
    event_rx: Receiver<AppEvent>,
}

impl Coordinator {
    pub fn new() -> Self {
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        Self {
            state: StateMachine::new(),
            event_tx,
            event_rx,
        }
    }

    pub fn sender(&self) -> Sender<AppEvent> {
        self.event_tx.clone()
    }

    pub fn state(&self) -> AppState {
        self.state.state()
    }

    pub fn handle(&mut self, event: AppEvent) -> Result<AppState, TransitionError> {
        self.state.apply(event)
    }

    pub fn drain(&mut self) -> Vec<Result<AppState, TransitionError>> {
        let mut results = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            results.push(self.handle(event));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::events::{DeliveryTarget, RecordingMode};

    #[test]
    fn synthetic_ptt_flow_via_handle() {
        let mut coord = Coordinator::new();
        coord
            .handle(AppEvent::StartRecording {
                mode: RecordingMode::PushToTalk,
            })
            .unwrap();
        coord.handle(AppEvent::StopRecording).unwrap();
        coord
            .handle(AppEvent::TranscriptReady {
                text: "test".into(),
                target: DeliveryTarget::InjectAtCaret,
            })
            .unwrap();
        coord.handle(AppEvent::DeliveryComplete).unwrap();
        assert_eq!(coord.state(), AppState::Idle);
    }
}
