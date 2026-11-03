//! Band-limited resampling (Hann-windowed sinc). Used to prepare the analysis. Not used on the audio thread.

use std::f64::consts::PI;

/// Converts `x` (`from` Hz) to `to` Hz. When `to < from`, the band is cut just below `to / 2`.
pub fn resample(x: &[f64], from: f64, to: f64) -> Vec<f64> {
    let ratio = to / from;
    let cutoff = ratio.min(1.0) * 0.95; // ratio to the input Nyquist
    let half_taps = 32.0 / cutoff;
    let n = (x.len() as f64 * ratio).floor() as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / ratio;
            let lo = (t - half_taps).ceil().max(0.0) as usize;
            let hi = ((t + half_taps).floor() as usize).min(x.len().saturating_sub(1));
            let mut acc = 0.0;
            for (j, v) in x.iter().enumerate().take(hi + 1).skip(lo) {
                let d = t - j as f64;
                let w = 0.5 + 0.5 * (PI * d / half_taps).cos();
                acc += v * cutoff * sinc(cutoff * d) * w;
            }
            acc
        })
        .collect()
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_tone_below_cutoff_and_removes_one_above() {
        let from = 48_000.0;
        let to = 11_000.0;
        let tone = |f: f64| (0..4800).map(|i| (2.0 * PI * f * i as f64 / from).sin()).collect::<Vec<_>>();
        let rms = |y: &[f64]| (y[200..y.len() - 200].iter().map(|v| v * v).sum::<f64>() / (y.len() - 400) as f64).sqrt();
        let kept = resample(&tone(1000.0), from, to);
        let removed = resample(&tone(9000.0), from, to);
        assert!((rms(&kept) - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01, "{}", rms(&kept));
        assert!(rms(&removed) < 0.01, "{}", rms(&removed));
    }
}
