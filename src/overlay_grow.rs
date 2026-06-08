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

#[inline]
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Width fraction (0..=1) for the recording pill while it grows in.
pub fn recording_width_fraction(
    show_start_ms: u64,
    now_ms: u64,
    asr_ready: bool,
    asr_ready_ms: Option<u64>,
) -> f32 {
    let elapsed = now_ms.saturating_sub(show_start_ms);
    let warmup_t = ease_out_cubic((elapsed as f32 / WARMUP_MS as f32).min(1.0));

    if asr_ready {
        let ready_ms = asr_ready_ms.unwrap_or(now_ms);
        let warmup_at_ready = ease_out_cubic(
            (ready_ms.saturating_sub(show_start_ms) as f32 / WARMUP_MS as f32).min(1.0),
        );
        let base = MIN_FRAC + (HOLD_FRAC - MIN_FRAC) * warmup_at_ready;
        let snap_t = ease_out_cubic((now_ms.saturating_sub(ready_ms) as f32 / SNAP_MS as f32).min(1.0));
        base + (1.0 - base) * snap_t
    } else {
        MIN_FRAC + (HOLD_FRAC - MIN_FRAC) * warmup_t
    }
}

pub fn display_width(fraction: f32) -> i32 {
    let w = (OVERLAY_WIDTH as f32 * fraction.clamp(0.0, 1.0)).round() as i32;
    w.clamp(PILL_MIN_WIDTH, OVERLAY_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_minimum_width() {
        let frac = recording_width_fraction(1_000, 1_000, false, None);
        assert_eq!(display_width(frac), PILL_MIN_WIDTH);
    }

    #[test]
    fn grows_partway_before_asr_ready() {
        let frac = recording_width_fraction(0, 420, false, None);
        assert!(frac > MIN_FRAC);
        assert!(frac <= HOLD_FRAC + 0.01);
        assert!(display_width(frac) < OVERLAY_WIDTH);
    }

    #[test]
    fn completes_to_full_width_after_asr_ready() {
        let frac = recording_width_fraction(0, 420 + SNAP_MS, true, Some(420));
        assert!((frac - 1.0).abs() < 0.02);
        assert_eq!(display_width(frac), OVERLAY_WIDTH);
    }

    #[test]
    fn immediate_asr_ready_still_animates_to_full() {
        let frac = recording_width_fraction(0, SNAP_MS, true, Some(0));
        assert!((frac - 1.0).abs() < 0.02);
    }
}
