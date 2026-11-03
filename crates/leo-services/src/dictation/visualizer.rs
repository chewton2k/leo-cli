use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::sync::Arc;

const DB_MIN: f32 = -68.0;
const DB_MAX: f32 = -30.0;
const GAIN: f32 = 1.3;
const CURVE_POWER: f32 = 0.7;
const WINDOW: usize = 512;
pub const BUCKETS: usize = 16;
const FREQ_MIN: f32 = 400.0;
const FREQ_MAX: f32 = 4000.0;
const SAMPLE_RATE: f32 = 16_000.0;

pub struct Visualizer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    ranges: Vec<(usize, usize)>,
    input: Vec<Complex32>,
    buffer: Vec<f32>,
}

impl Default for Visualizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Visualizer {
    pub fn new() -> Self {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let window = (0..WINDOW)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / WINDOW as f32).cos()))
            .collect();
        let nyquist = SAMPLE_RATE / 2.0;
        let (low, high) = (FREQ_MIN.min(nyquist), FREQ_MAX.min(nyquist));
        let ranges = (0..BUCKETS)
            .map(|b| {
                let start = (b as f32 / BUCKETS as f32).powi(2);
                let end = ((b + 1) as f32 / BUCKETS as f32).powi(2);
                let start_hz = low + (high - low) * start;
                let end_hz = low + (high - low) * end;
                let first = ((start_hz * WINDOW as f32) / SAMPLE_RATE) as usize;
                let mut last = ((end_hz * WINDOW as f32) / SAMPLE_RATE) as usize;
                if last <= first {
                    last = first + 1;
                }
                (first.min(WINDOW / 2), last.min(WINDOW / 2))
            })
            .collect();
        Self {
            fft,
            window,
            ranges,
            input: vec![Complex32::new(0.0, 0.0); WINDOW],
            buffer: Vec::with_capacity(WINDOW * 2),
        }
    }

    pub fn feed(&mut self, samples: &[f32]) -> Option<Vec<f32>> {
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() < WINDOW {
            return None;
        }
        let frame = &self.buffer[..WINDOW];
        let mean = frame.iter().sum::<f32>() / WINDOW as f32;
        for (i, sample) in frame.iter().enumerate() {
            self.input[i] = Complex32::new((sample - mean) * self.window[i], 0.0);
        }
        self.fft.process(&mut self.input);
        let mut buckets = vec![0.0; BUCKETS];
        for (bucket, &(first, last)) in buckets.iter_mut().zip(&self.ranges) {
            if first >= last {
                continue;
            }
            let power = self.input[first..last]
                .iter()
                .map(|bin| bin.norm_sqr())
                .sum::<f32>()
                / (last - first) as f32;
            let db = if power > 1e-12 {
                20.0 * (power.sqrt() / WINDOW as f32).log10()
            } else {
                -80.0
            };
            let normalized = ((db - DB_MIN) / (DB_MAX - DB_MIN)).clamp(0.0, 1.0);
            *bucket = (normalized * GAIN).powf(CURVE_POWER).clamp(0.0, 1.0);
        }
        for i in 1..BUCKETS - 1 {
            buckets[i] = buckets[i] * 0.7 + buckets[i - 1] * 0.15 + buckets[i + 1] * 0.15;
        }
        self.buffer.clear();
        Some(buckets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, samples: usize, amplitude: f32) -> Vec<f32> {
        (0..samples)
            .map(|n| amplitude * (2.0 * std::f32::consts::PI * hz * n as f32 / SAMPLE_RATE).sin())
            .collect()
    }

    #[test]
    fn levels_arrive_once_per_window() {
        let mut v = Visualizer::new();
        assert!(v.feed(&[0.0; WINDOW - 1]).is_none());
        let levels = v.feed(&[0.0; 1]).unwrap();
        assert_eq!(levels.len(), BUCKETS);
        assert!(levels.iter().all(|l| *l == 0.0));
        assert!(v.feed(&[0.0; 10]).is_none());
    }

    #[test]
    fn speech_band_tones_raise_the_bars_and_stay_in_range() {
        let mut v = Visualizer::new();
        let quiet = v.feed(&tone(1000.0, WINDOW, 0.0005)).unwrap();
        let loud = v.feed(&tone(1000.0, WINDOW, 0.3)).unwrap();
        assert!(loud.iter().all(|l| (0.0..=1.0).contains(l)));
        let peak = |levels: &[f32]| levels.iter().cloned().fold(0.0, f32::max);
        assert!(peak(&quiet) < 0.2, "{quiet:?}");
        assert!(peak(&loud) > 0.6, "{loud:?}");
    }
}
