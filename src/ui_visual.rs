//! Shared dictation UI phase and pill palette (testable without Win32).

use crate::app::AppState;
use crate::timing;

/// User-visible dictation phase — mirrors runtime + mic-arm state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPhase {
    Hidden,
    /// Win+Ctrl held, mic pre-roll, before 300 ms PTT threshold.
    Armed,
    RecordingPtt,
    RecordingHandsFree,
    Processing,
}

/// Tray icon affordance (coarser than overlay — no PTT vs hands-free split).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconState {
    Idle,
    Armed,
    Recording,
    Processing,
}

/// Pill plasma palette (matches overlay `plasma_rgb` mid/high tones).
pub const PILL_RING: (u8, u8, u8) = (98, 10, 82);
pub const PILL_RING_BRIGHT: (u8, u8, u8) = (150, 40, 118);
pub const PILL_RING_DIM: (u8, u8, u8) = (48, 0, 58);
pub const PILL_CORE: (u8, u8, u8) = (14, 0, 22);

pub fn ui_phase(app: AppState, mic_armed: bool) -> UiPhase {
    match app {
        AppState::RecordingPtt => UiPhase::RecordingPtt,
        AppState::RecordingHandsFree => UiPhase::RecordingHandsFree,
        AppState::Processing | AppState::Injecting => UiPhase::Processing,
        _ if mic_armed => UiPhase::Armed,
        _ => UiPhase::Hidden,
    }
}

pub fn tray_icon_state(phase: UiPhase) -> TrayIconState {
    match phase {
        UiPhase::Hidden => TrayIconState::Idle,
        UiPhase::Armed => TrayIconState::Armed,
        UiPhase::RecordingPtt | UiPhase::RecordingHandsFree => TrayIconState::Recording,
        UiPhase::Processing => TrayIconState::Processing,
    }
}

/// 0..=1 progress toward PTT threshold while armed.
pub fn ptt_hold_fraction(arm_start_ms: u64, now_ms: u64) -> f32 {
    let elapsed = now_ms.saturating_sub(arm_start_ms);
    (elapsed as f32 / timing::PTT_MIN_HOLD_MS as f32).clamp(0.0, 1.0)
}

pub fn phase_uses_grow_animation(phase: UiPhase) -> bool {
    matches!(
        phase,
        UiPhase::Armed | UiPhase::RecordingPtt | UiPhase::RecordingHandsFree
    )
}

/// Armed / waiting: 40% smaller than active recording or processing.
pub const PILL_INACTIVE_SCALE: f32 = 0.6;

/// Visual scale for the bottom-center pill (width + height).
pub fn phase_display_scale(phase: UiPhase) -> f32 {
    if matches!(phase, UiPhase::Armed) {
        PILL_INACTIVE_SCALE
    } else {
        1.0
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}

pub(crate) fn scale_rgb((r, g, b): (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    (
        (r as f32 * factor).min(255.0) as u8,
        (g as f32 * factor).min(255.0) as u8,
        (b as f32 * factor).min(255.0) as u8,
    )
}

/// 16×16 RGBA tray icon pixels (pill purple tones).
pub fn tray_icon_rgba(state: TrayIconState, size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0 - 0.5;
    let outer = center - 0.5;
    let inner = outer * 0.62;

    let (outer_rgb, inner_rgb, core_rgb) = match state {
        TrayIconState::Idle => (PILL_RING, PILL_RING_DIM, PILL_CORE),
        TrayIconState::Armed => (
            scale_rgb(PILL_RING, 0.72),
            scale_rgb(PILL_RING_DIM, 0.85),
            PILL_CORE,
        ),
        TrayIconState::Recording => (PILL_RING_BRIGHT, PILL_RING, PILL_CORE),
        TrayIconState::Processing => (
            PILL_RING,
            scale_rgb(PILL_RING_BRIGHT, 0.75),
            scale_rgb(PILL_RING_DIM, 0.9),
        ),
    };

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let i = ((y * size + x) * 4) as usize;
            let (r, g, b) = if dist <= inner {
                core_rgb
            } else if dist <= outer {
                inner_rgb
            } else if dist <= outer + 1.2 {
                outer_rgb
            } else {
                continue;
            };
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 0xFF;
        }
    }

    rgba
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;

    #[test]
    fn armed_when_mic_armed_and_idle() {
        assert_eq!(ui_phase(AppState::Idle, true), UiPhase::Armed);
    }

    #[test]
    fn recording_overrides_armed_flag() {
        assert_eq!(
            ui_phase(AppState::RecordingPtt, true),
            UiPhase::RecordingPtt
        );
    }

    #[test]
    fn tray_maps_phases() {
        assert_eq!(tray_icon_state(UiPhase::Armed), TrayIconState::Armed);
        assert_eq!(
            tray_icon_state(UiPhase::RecordingHandsFree),
            TrayIconState::Recording
        );
    }

    #[test]
    fn ptt_hold_reaches_one_at_threshold() {
        let frac = ptt_hold_fraction(0, timing::PTT_MIN_HOLD_MS);
        assert!((frac - 1.0).abs() < 0.01);
    }

    #[test]
    fn armed_uses_inactive_scale() {
        assert!((phase_display_scale(UiPhase::Armed) - PILL_INACTIVE_SCALE).abs() < f32::EPSILON);
        assert!((phase_display_scale(UiPhase::RecordingPtt) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn tray_icon_has_opaque_ring_pixels() {
        let px = tray_icon_rgba(TrayIconState::Idle, 16);
        let colored = px.chunks(4).filter(|p| p[3] > 0).count();
        assert!(colored > 20);
    }

    #[test]
    fn lerp_rgb_endpoints() {
        assert_eq!(lerp_rgb((0, 0, 0), (100, 200, 50), 0.0), (0, 0, 0));
        assert_eq!(lerp_rgb((0, 0, 0), (100, 200, 50), 1.0), (100, 200, 50));
    }

    #[test]
    fn scale_rgb_clamps_to_byte_max() {
        assert_eq!(scale_rgb((200, 200, 200), 2.0), (255, 255, 255));
    }
}
