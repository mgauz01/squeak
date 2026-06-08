//! Horizontal pill grow animation (pure Rust, unit-testable).

/// Full pill width in pixels.
pub const OVERLAY_WIDTH: i32 = 126;
/// Slightly taller capsule for softer, rounder ends.
pub const OVERLAY_HEIGHT: i32 = 36;
/// Starting width when the pill first appears.
pub const PILL_MIN_WIDTH: i32 = 28;
/// Corner ellipse diameter — full height gives a true capsule.
pub const PILL_CORNER: i32 = OVERLAY_HEIGHT;

const MIN_FRAC: f32 = PILL_MIN_WIDTH as f32 / OVERLAY_WIDTH as f32;
const HOLD_FRAC: f32 = 0.74;
const WARMUP_MS: u64 = 420;
const SNAP_MS: u64 = 160;
pub const SCALE_TRANSITION_MS: u64 = 220;

#[inline]
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Width fraction (0..=1) for the recording pill while it grows in.
///
/// When `allow_full_grow` is false (armed / inactive), growth stops at `HOLD_FRAC`
/// even after ASR is ready — full width waits until recording starts.
pub fn recording_width_fraction(
    show_start_ms: u64,
    now_ms: u64,
    asr_ready: bool,
    asr_ready_ms: Option<u64>,
    allow_full_grow: bool,
) -> f32 {
    let elapsed = now_ms.saturating_sub(show_start_ms);
    let warmup_t = ease_out_cubic((elapsed as f32 / WARMUP_MS as f32).min(1.0));
    let warmup_frac = MIN_FRAC + (HOLD_FRAC - MIN_FRAC) * warmup_t;

    if asr_ready && allow_full_grow {
        let ready_ms = asr_ready_ms.unwrap_or(now_ms);
        let warmup_at_ready = ease_out_cubic(
            (ready_ms.saturating_sub(show_start_ms) as f32 / WARMUP_MS as f32).min(1.0),
        );
        let base = MIN_FRAC + (HOLD_FRAC - MIN_FRAC) * warmup_at_ready;
        let snap_t =
            ease_out_cubic((now_ms.saturating_sub(ready_ms) as f32 / SNAP_MS as f32).min(1.0));
        base + (1.0 - base) * snap_t
    } else {
        warmup_frac.min(HOLD_FRAC)
    }
}

pub fn display_width(fraction: f32) -> i32 {
    let w = (OVERLAY_WIDTH as f32 * fraction.clamp(0.0, 1.0)).round() as i32;
    w.clamp(PILL_MIN_WIDTH, OVERLAY_WIDTH)
}

pub fn scaled_dimension(base: i32, scale: f32) -> i32 {
    ((base as f32) * scale.clamp(0.0, 1.0)).round() as i32
}

/// Ease between two scale endpoints (e.g. inactive 0.6 → active 1.0).
pub fn animated_scale(from: f32, to: f32, start_ms: u64, now_ms: u64) -> f32 {
    if start_ms == 0 || (from - to).abs() < f32::EPSILON {
        return to;
    }
    let elapsed = now_ms.saturating_sub(start_ms);
    let t = ease_out_cubic((elapsed as f32 / SCALE_TRANSITION_MS as f32).min(1.0));
    from + (to - from) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_minimum_width() {
        let frac = recording_width_fraction(1_000, 1_000, false, None, true);
        assert_eq!(display_width(frac), PILL_MIN_WIDTH);
    }

    #[test]
    fn grows_partway_before_asr_ready() {
        let frac = recording_width_fraction(0, 420, false, None, true);
        assert!(frac > MIN_FRAC);
        assert!(frac <= HOLD_FRAC + 0.01);
        assert!(display_width(frac) < OVERLAY_WIDTH);
    }

    #[test]
    fn completes_to_full_width_after_asr_ready() {
        let frac = recording_width_fraction(0, 420 + SNAP_MS, true, Some(420), true);
        assert!((frac - 1.0).abs() < 0.02);
        assert_eq!(display_width(frac), OVERLAY_WIDTH);
    }

    #[test]
    fn immediate_asr_ready_still_animates_to_full() {
        let frac = recording_width_fraction(0, SNAP_MS, true, Some(0), true);
        assert!((frac - 1.0).abs() < 0.02);
    }

    #[test]
    fn armed_caps_grow_when_asr_ready() {
        let frac = recording_width_fraction(0, 420 + SNAP_MS, true, Some(420), false);
        assert!(frac <= HOLD_FRAC + 0.01);
        assert!(frac < 1.0);
    }

    #[test]
    fn scaled_dimension_shrinks_by_inactive_factor() {
        assert_eq!(scaled_dimension(OVERLAY_WIDTH, 0.6), 76);
        assert_eq!(scaled_dimension(OVERLAY_HEIGHT, 0.6), 22);
    }
}
