//! F1–F4 estimation with the same steps as Praat: resampling to twice the ceiling, pre-emphasis from 50 Hz,
//! a Gaussian window, Burg LPC, and frequencies and bandwidths from the polynomial roots.

use rustfft::num_complex::Complex64;
use std::f64::consts::PI;
use voice_domain::ports::FormantEstimator;
use voice_domain::{AnalysisSettings, Formant, Frequency, SampleRate};

use crate::resample::resample;

/// As in Praat's default, the actual window length is 50 ms (twice Praat's "window length 0.025").
pub const LPC_WINDOW_SECONDS: f64 = 0.05;

#[derive(Default)]
pub struct BurgFormants;

impl FormantEstimator for BurgFormants {
    fn estimate(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> [Option<Formant>; 4] {
        lowest_four(self.candidates(window, rate, settings))
    }
}

impl BurgFormants {
    /// Returns all upper-half-plane LPC roots between 50 Hz and ceiling−50 Hz for the middle 50 ms of the window, in frequency order (the tracking candidates).
    pub fn candidates(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> Vec<Formant> {
        let fs = rate.hz() as f64;
        let len = ((LPC_WINDOW_SECONDS * fs) as usize).min(window.len());
        let start = (window.len() - len) / 2;
        let x: Vec<f64> = window[start..start + len].iter().map(|v| *v as f64).collect();
        let ceiling = settings.ceiling.hz() as f64;
        let fs2 = 2.0 * ceiling;
        let y = prepare(&resample(&x, fs, fs2), fs2);
        let a = burg(&y, settings.lpc_order.get() as usize);
        candidates_from_lpc(&a, fs2, ceiling)
    }
}

/// Pre-emphasis and the Gaussian window.
fn prepare(x: &[f64], fs: f64) -> Vec<f64> {
    let alpha = (-2.0 * PI * 50.0 / fs).exp();
    let n = x.len();
    let e12 = (-12.0f64).exp();
    (0..n)
        .map(|i| {
            let emphasized = x[i] - if i > 0 { alpha * x[i - 1] } else { 0.0 };
            let t = (i as f64 + 0.5) / n as f64 - 0.5;
            let w = ((-48.0 * t * t).exp() - e12) / (1.0 - e12);
            emphasized * w
        })
        .collect()
}

/// Burg's method. Returns the coefficients of the prediction error filter A(z) = Σ a[k] z^-k, with `a[0] = 1`.
pub fn burg(x: &[f64], order: usize) -> Vec<f64> {
    let n = x.len();
    let mut a = vec![0.0; order + 1];
    a[0] = 1.0;
    let mut f = x.to_vec();
    let mut b = x.to_vec();
    for m in 1..=order {
        let (mut num, mut den) = (0.0, 0.0);
        for i in m..n {
            num += f[i] * b[i - 1];
            den += f[i] * f[i] + b[i - 1] * b[i - 1];
        }
        if den <= 0.0 {
            break;
        }
        let k = -2.0 * num / den;
        let prev = a.clone();
        for i in 1..=m {
            a[i] = prev[i] + k * prev[m - i];
        }
        for i in (m..n).rev() {
            let fi = f[i];
            f[i] = fi + k * b[i - 1];
            b[i] = b[i - 1] + k * fi;
        }
    }
    a
}

/// Roots of the polynomial z^p + a1 z^(p-1) + … + ap (Aberth method).
pub fn roots(a: &[f64]) -> Vec<Complex64> {
    let p = a.len() - 1;
    if p == 0 {
        return Vec::new();
    }
    let eval = |z: Complex64| -> (Complex64, Complex64) {
        let mut v = Complex64::new(1.0, 0.0);
        let mut d = Complex64::new(0.0, 0.0);
        for c in &a[1..] {
            d = d * z + v;
            v = v * z + c;
        }
        (v, d)
    };
    let radius = 1.0 + a[1..].iter().fold(0.0f64, |m, c| m.max(c.abs()));
    let mut z: Vec<Complex64> = (0..p)
        .map(|k| Complex64::from_polar(radius.min(1.5) * 0.9, 2.0 * PI * (k as f64 + 0.25) / p as f64))
        .collect();
    for _ in 0..500 {
        let mut max_step = 0.0f64;
        for i in 0..p {
            let (v, d) = eval(z[i]);
            if v.norm() == 0.0 {
                continue;
            }
            let ratio = v / d;
            let sum: Complex64 = (0..p).filter(|&j| j != i).map(|j| Complex64::new(1.0, 0.0) / (z[i] - z[j])).sum();
            let step = ratio / (Complex64::new(1.0, 0.0) - ratio * sum);
            z[i] -= step;
            max_step = max_step.max(step.norm());
        }
        if max_step < 1e-12 {
            break;
        }
    }
    z
}

/// Sorts the upper-half-plane roots between 50 Hz and ceiling−50 Hz by frequency and returns the lowest 4.
pub fn formants_from_lpc(a: &[f64], fs: f64, ceiling: f64) -> [Option<Formant>; 4] {
    lowest_four(candidates_from_lpc(a, fs, ceiling))
}

/// Returns all upper-half-plane roots between 50 Hz and ceiling−50 Hz as frequencies and bandwidths, in frequency order.
pub fn candidates_from_lpc(a: &[f64], fs: f64, ceiling: f64) -> Vec<Formant> {
    let mut found: Vec<(f64, f64)> = roots(a)
        .into_iter()
        .filter(|z| z.im > 0.0)
        .map(|z| {
            // Reflect roots outside the unit circle inside (as Praat does)
            let r = z.norm();
            let r = if r > 1.0 { 1.0 / r } else { r };
            let f = z.arg() * fs / (2.0 * PI);
            let bw = -r.ln() * fs / PI;
            (f, bw)
        })
        .filter(|(f, _)| *f > 50.0 && *f < ceiling - 50.0)
        .collect();
    found.sort_by(|x, y| x.0.total_cmp(&y.0));
    found
        .into_iter()
        .filter_map(|(f, bw)| match (Frequency::new(f as f32), Frequency::new(bw.max(1.0) as f32)) {
            (Ok(frequency), Ok(bandwidth)) => Some(Formant { frequency, bandwidth }),
            _ => None,
        })
        .collect()
}

/// Makes the lowest 4 of the frequency-ordered candidates F1–F4 (the earlier method, without tracking).
pub fn lowest_four(candidates: Vec<Formant>) -> [Option<Formant>; 4] {
    let mut out = [None; 4];
    for (slot, c) in out.iter_mut().zip(candidates) {
        *slot = Some(c);
    }
    out
}

/// The level of the LPC envelope (dB, relative) at frequency `f`.
pub fn envelope_db(a: &[f64], fs: f64, f: f64) -> f64 {
    let w = 2.0 * PI * f / fs;
    let mut re = 0.0;
    let mut im = 0.0;
    for (k, c) in a.iter().enumerate() {
        re += c * (k as f64 * w).cos();
        im -= c * (k as f64 * w).sin();
    }
    -10.0 * (re * re + im * im).max(1e-30).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_of_known_polynomial() {
        // (z - 0.5)(z + 0.25) = z^2 - 0.25 z - 0.125
        let mut r: Vec<f64> = roots(&[1.0, -0.25, -0.125]).iter().map(|z| z.re).collect();
        r.sort_by(f64::total_cmp);
        assert!((r[0] + 0.25).abs() < 1e-9 && (r[1] - 0.5).abs() < 1e-9, "{r:?}");
    }

    /// Burg's method recovers the resonance from white noise passed through a known second-order resonance.
    #[test]
    fn burg_recovers_a_single_resonance() {
        let fs = 11_000.0;
        let (f, bw) = (1000.0, 100.0);
        let r = (-PI * bw / fs).exp();
        let (a1, a2) = (2.0 * r * (2.0 * PI * f / fs).cos(), -r * r);
        let mut state = 12345u64;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let (mut y1, mut y2) = (0.0, 0.0);
        let x: Vec<f64> = (0..4000)
            .map(|_| {
                let y = noise() + a1 * y1 + a2 * y2;
                y2 = y1;
                y1 = y;
                y
            })
            .collect();
        let a = burg(&x, 2);
        let fm = formants_from_lpc(&a, fs, 5000.0);
        let got = fm[0].expect("one found");
        assert!((got.frequency.hz() as f64 / f - 1.0).abs() < 0.02, "{got:?}");
        assert!((got.bandwidth.hz() as f64 - bw).abs() < 40.0, "{got:?}");
    }
}
