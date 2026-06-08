//! Win+Ctrl gesture finite-state machine (portable, unit-tested).
//!
//! Planning defaults: 300 ms PTT min hold, 400 ms double-tap window.

use crate::timing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureOutput {
    None,
    /// Win+Ctrl pressed — start mic pre-roll before the 300 ms PTT threshold.
    ArmMicrophone,
    /// Short tap released before PTT — discard pre-roll without processing.
    DisarmMicrophone,
    StartPushToTalk,
    StopPushToTalk,
    StartHandsFree,
    StopHandsFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    ComboHeld {
        since_ms: u64,
        is_second_tap: bool,
        /// When true, completing a double-tap stops hands-free instead of starting it.
        toggling_off_hands_free: bool,
    },
    PushToTalk,
    HandsFree,
    AwaitingSecondTap {
        first_release_ms: u64,
        toggling_off_hands_free: bool,
    },
}

pub struct GestureFsm {
    ptt_min_hold_ms: u64,
    double_tap_window_ms: u64,
    phase: Phase,
    win_down: bool,
    ctrl_down: bool,
}

impl Default for GestureFsm {
    fn default() -> Self {
        Self::new(timing::PTT_MIN_HOLD_MS, timing::DOUBLE_TAP_WINDOW_MS)
    }
}

impl GestureFsm {
    pub fn new(ptt_min_hold_ms: u64, double_tap_window_ms: u64) -> Self {
        Self {
            ptt_min_hold_ms,
            double_tap_window_ms,
            phase: Phase::Idle,
            win_down: false,
            ctrl_down: false,
        }
    }

    fn combo_active(&self) -> bool {
        self.win_down && self.ctrl_down
    }

    pub fn on_tick(&mut self, now_ms: u64) -> GestureOutput {
        if let Phase::AwaitingSecondTap {
            first_release_ms, ..
        } = self.phase
        {
            if now_ms.saturating_sub(first_release_ms) > self.double_tap_window_ms {
                self.phase = Phase::Idle;
            }
            return GestureOutput::None;
        }

        if let Phase::ComboHeld { since_ms, .. } = self.phase {
            if self.combo_active() && now_ms.saturating_sub(since_ms) >= self.ptt_min_hold_ms {
                self.phase = Phase::PushToTalk;
                return GestureOutput::StartPushToTalk;
            }
        }

        GestureOutput::None
    }

    pub fn on_key(&mut self, key: Key, pressed: bool, now_ms: u64) -> GestureOutput {
        match key {
            Key::Win => self.win_down = pressed,
            Key::Ctrl => self.ctrl_down = pressed,
        }

        if self.combo_active() {
            self.on_combo_down(now_ms)
        } else {
            self.on_combo_up(now_ms)
        }
    }

    fn on_combo_down(&mut self, now_ms: u64) -> GestureOutput {
        match self.phase {
            Phase::Idle => {
                self.phase = Phase::ComboHeld {
                    since_ms: now_ms,
                    is_second_tap: false,
                    toggling_off_hands_free: false,
                };
                GestureOutput::ArmMicrophone
            }
            Phase::AwaitingSecondTap {
                toggling_off_hands_free,
                ..
            } => {
                self.phase = Phase::ComboHeld {
                    since_ms: now_ms,
                    is_second_tap: true,
                    toggling_off_hands_free,
                };
                GestureOutput::ArmMicrophone
            }
            Phase::HandsFree => {
                self.phase = Phase::ComboHeld {
                    since_ms: now_ms,
                    is_second_tap: false,
                    toggling_off_hands_free: true,
                };
                GestureOutput::ArmMicrophone
            }
            Phase::ComboHeld { .. } | Phase::PushToTalk => GestureOutput::None,
        }
    }

    fn on_combo_up(&mut self, now_ms: u64) -> GestureOutput {
        match self.phase {
            Phase::ComboHeld {
                since_ms,
                is_second_tap: false,
                toggling_off_hands_free,
            } => {
                if now_ms.saturating_sub(since_ms) < self.ptt_min_hold_ms {
                    self.phase = Phase::AwaitingSecondTap {
                        first_release_ms: now_ms,
                        toggling_off_hands_free,
                    };
                    GestureOutput::DisarmMicrophone
                } else {
                    self.phase = Phase::Idle;
                    GestureOutput::None
                }
            }
            Phase::ComboHeld {
                since_ms,
                is_second_tap: true,
                toggling_off_hands_free,
            } => {
                if now_ms.saturating_sub(since_ms) < self.ptt_min_hold_ms {
                    if toggling_off_hands_free {
                        self.phase = Phase::Idle;
                        GestureOutput::StopHandsFree
                    } else {
                        self.phase = Phase::HandsFree;
                        GestureOutput::StartHandsFree
                    }
                } else {
                    self.phase = Phase::Idle;
                    GestureOutput::None
                }
            }
            Phase::PushToTalk => {
                self.phase = Phase::Idle;
                GestureOutput::StopPushToTalk
            }
            Phase::Idle | Phase::AwaitingSecondTap { .. } | Phase::HandsFree => GestureOutput::None,
        }
    }

    pub fn simulate_tap(&mut self, at_ms: u64) -> Vec<GestureOutput> {
        let mut outputs = Vec::new();
        push(
            &mut outputs,
            [
                self.on_key(Key::Win, true, at_ms),
                self.on_key(Key::Ctrl, true, at_ms),
                self.on_key(Key::Ctrl, false, at_ms + 1),
                self.on_key(Key::Win, false, at_ms + 2),
            ],
        );
        outputs
    }

    pub fn simulate_hold(&mut self, start_ms: u64, release_ms: u64) -> Vec<GestureOutput> {
        let mut outputs = Vec::new();
        push(
            &mut outputs,
            [
                self.on_key(Key::Win, true, start_ms),
                self.on_key(Key::Ctrl, true, start_ms),
            ],
        );

        let mut t = start_ms;
        while t <= release_ms {
            push(&mut outputs, [self.on_tick(t)]);
            t += 50;
        }

        push(
            &mut outputs,
            [
                self.on_key(Key::Ctrl, false, release_ms),
                self.on_key(Key::Win, false, release_ms + 1),
            ],
        );
        outputs
    }
}

fn push(out: &mut Vec<GestureOutput>, items: impl IntoIterator<Item = GestureOutput>) {
    for item in items {
        if item != GestureOutput::None {
            out.push(item);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Win,
    Ctrl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_tap_arms_and_disarms_without_ptt() {
        let mut fsm = GestureFsm::default();
        let out = fsm.simulate_tap(0);
        assert!(out.contains(&GestureOutput::ArmMicrophone));
        assert!(out.contains(&GestureOutput::DisarmMicrophone));
        assert!(!out.contains(&GestureOutput::StartPushToTalk));
    }

    #[test]
    fn hold_arms_then_starts_ptt() {
        let mut fsm = GestureFsm::default();
        let mut out = Vec::new();
        push(
            &mut out,
            [
                fsm.on_key(Key::Win, true, 0),
                fsm.on_key(Key::Ctrl, true, 0),
            ],
        );
        assert!(out.contains(&GestureOutput::ArmMicrophone));
        push(&mut out, [fsm.on_tick(300)]);
        assert!(out.contains(&GestureOutput::StartPushToTalk));
        assert!(!out.contains(&GestureOutput::DisarmMicrophone));
    }

    #[test]
    fn hold_triggers_ptt_start_and_stop() {
        let mut fsm = GestureFsm::default();
        let out = fsm.simulate_hold(0, 350);
        assert!(out.contains(&GestureOutput::StartPushToTalk));
        assert!(out.contains(&GestureOutput::StopPushToTalk));
    }

    #[test]
    fn double_tap_starts_hands_free() {
        let mut fsm = GestureFsm::default();
        let _ = fsm.simulate_tap(0);
        let out = fsm.simulate_tap(100);
        assert!(out.contains(&GestureOutput::StartHandsFree));
    }

    #[test]
    fn double_tap_again_stops_hands_free() {
        let mut fsm = GestureFsm::default();
        let _ = fsm.simulate_tap(0);
        let _ = fsm.simulate_tap(100);
        let _ = fsm.simulate_tap(200);
        let out = fsm.simulate_tap(300);
        assert!(out.contains(&GestureOutput::StopHandsFree));
    }

    #[test]
    fn sustained_hold_does_not_toggle_hands_free() {
        let mut fsm = GestureFsm::default();
        let out = fsm.simulate_hold(0, 500);
        assert!(out.contains(&GestureOutput::StartPushToTalk));
        assert!(!out.contains(&GestureOutput::StartHandsFree));
    }
}
