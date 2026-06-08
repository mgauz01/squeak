#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingMode {
    PushToTalk,
    HandsFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryTarget {
    InjectAtCaret,
    ClipboardFallback,
    BufferWithToast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAction {
    Exit,
    OpenSettings,
    PasteLast,
    SetAsrModel(String),
    /// Moonshine tier shortcut (`tiny` / `small` / `medium`).
    SetModelTier(String),
    /// Grammar profile: `off`, `tiny`, `coedit`, or `llama` (feature-gated).
    SetGrammarProfile(String),
    ToggleAutostart(bool),
    ToggleDirectMl(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    ArmRecording,
    DisarmRecording,
    StartRecording {
        mode: RecordingMode,
    },
    StopRecording,
    CancelRecording,
    TranscriptReady {
        text: String,
        target: DeliveryTarget,
    },
    ProcessingFailed {
        message: String,
    },
    DeliveryComplete,
    DeliveryBuffered,
    DismissError,
    UserAction(UserAction),
    SecondInstanceWake,
    /// ASR worker finished loading the speech model (overlay grow animation).
    AsrModelReady,
}
