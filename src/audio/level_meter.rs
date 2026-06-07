use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const BAR_COUNT: usize = 5;

/// Live mic levels for the recording overlay (updated from the cpal callback).
pub struct AudioLevelMeter {
    bars: [AtomicU32; BAR_COUNT],
}

impl AudioLevelMeter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            bars: std::array::from_fn(|_| AtomicU32::new(0)),
        })
    }

    pub fn reset(&self) {
        for bar in &self.bars {
            bar.store(0, Ordering::Relaxed);
        }
    }

    pub fn bar_levels(&self) -> [f32; BAR_COUNT] {
        std::array::from_fn(|i| load_f32(&self.bars[i]))
    }

    /// Peak levels per band from the latest input chunk (0..1, smoothed).
    pub fn update_from_chunk(&self, samples: &[f32]) {
        if samples.is_empty() {
            self.decay_all();
            return;
        }

        let seg_len = (samples.len() / BAR_COUNT).max(1);
        for (i, bar) in self.bars.iter().enumerate() {
            let start = i * seg_len;
            let end = if i == BAR_COUNT - 1 {
                samples.len()
            } else {
                (start + seg_len).min(samples.len())
            };
            let peak = samples[start..end]
                .iter()
                .map(|s| s.abs())
                .fold(0.0_f32, f32::max);
            let scaled = (peak * 2.8).clamp(0.0, 1.0);
            let prev = load_f32(bar);
            let next = smooth_level(prev, scaled);
            store_f32(bar, next);
        }
    }

    fn decay_all(&self) {
        for bar in &self.bars {
            let prev = load_f32(bar);
            store_f32(bar, prev * 0.82);
        }
    }
}

fn smooth_level(current: f32, target: f32) -> f32 {
    let rate = if target > current { 0.5 } else { 0.18 };
    current + (target - current) * rate
}

fn store_f32(atom: &AtomicU32, v: f32) {
    atom.store(v.to_bits(), Ordering::Relaxed);
}

fn load_f32(atom: &AtomicU32) -> f32 {
    f32::from_bits(atom.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_updates_bar_levels() {
        let meter = AudioLevelMeter::new();
        let loud = vec![0.8_f32; 500];
        meter.update_from_chunk(&loud);
        let levels = meter.bar_levels();
        assert!(levels.iter().all(|&l| l > 0.4));

        meter.reset();
        let quiet = vec![0.01_f32; 500];
        meter.update_from_chunk(&quiet);
        let levels = meter.bar_levels();
        assert!(levels.iter().all(|&l| l < 0.2));
    }

    #[test]
    fn empty_chunk_decays_levels() {
        let meter = AudioLevelMeter::new();
        meter.update_from_chunk(&[0.9; 100]);
        assert!(meter.bar_levels()[0] > 0.5);
        meter.update_from_chunk(&[]);
        assert!(meter.bar_levels()[0] < 0.5);
    }
}
