//! F0 estimation with YIN (de Cheveigné & Kawahara 2002).

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use voice_domain::ports::PitchEstimator;
use voice_domain::{Frequency, SampleRate};

pub struct Yin {
    pub min_f0: f32,
    pub max_f0: f32,
    /// Threshold of the cumulative-mean-normalised difference function.
    pub threshold: f32,
    /// An RMS (dBFS) below this is unvoiced.
    pub silence_dbfs: f32,
    diff: Vec<f32>,
    fft: Option<(usize, Arc<dyn Fft<f32>>, Arc<dyn Fft<f32>>)>,
    a: Vec<Complex32>,
    b: Vec<Complex32>,
    square_prefix: Vec<f32>,
}

impl Default for Yin {
    fn default() -> Self {
        Self {
            min_f0: 40.0,
            max_f0: 1000.0,
            threshold: 0.15,
            silence_dbfs: -55.0,
            diff: Vec::new(),
            fft: None,
            a: Vec::new(),
            b: Vec::new(),
            square_prefix: Vec::new(),
        }
    }
}

impl Yin {
    /// Finds d(τ) = Σ_{j<w} (x_j − x_{j+τ})² with an FFT cross-correlation (d = e₀ + e(τ) − 2 r(τ)).
    fn difference(&mut self, x: &[f32], w: usize, max_tau: usize) {
        let n = (w + max_tau + 1).next_power_of_two() * 2;
        if self.fft.as_ref().map(|f| f.0) != Some(n) {
            let mut planner = FftPlanner::new();
            self.fft = Some((n, planner.plan_fft_forward(n), planner.plan_fft_inverse(n)));
        }
        let (_, fwd, inv) = self.fft.as_ref().expect("prepared");
        self.a.clear();
        self.a.resize(n, Complex32::default());
        self.b.clear();
        self.b.resize(n, Complex32::default());
        for j in 0..w {
            self.a[j].re = x[j];
        }
        for (j, v) in x.iter().enumerate().take(w + max_tau) {
            self.b[j].re = *v;
        }
        fwd.process(&mut self.a);
        fwd.process(&mut self.b);
        for (a, b) in self.a.iter_mut().zip(&self.b) {
            *a = a.conj() * b;
        }
        inv.process(&mut self.a);
        let scale = 1.0 / n as f32;
        self.square_prefix.clear();
        self.square_prefix.push(0.0);
        let mut acc = 0.0f32;
        for v in x.iter().take(w + max_tau) {
            acc += v * v;
            self.square_prefix.push(acc);
        }
        let e0 = self.square_prefix[w];
        self.diff.clear();
        self.diff.resize(max_tau + 1, 0.0);
        for tau in 1..=max_tau {
            let e_tau = self.square_prefix[tau + w] - self.square_prefix[tau];
            let r = self.a[tau].re * scale;
            self.diff[tau] = (e0 + e_tau - 2.0 * r).max(0.0);
        }
    }
}

impl PitchEstimator for Yin {
    fn estimate(&mut self, window: &[f32], rate: SampleRate) -> Option<Frequency> {
        let fs = rate.hz() as f32;
        let rms = (window.iter().map(|v| v * v).sum::<f32>() / window.len().max(1) as f32).sqrt();
        if 20.0 * rms.max(1e-12).log10() < self.silence_dbfs {
            return None;
        }
        let tau_min = (fs / self.max_f0).floor() as usize;
        let tau_max = (fs / self.min_f0).ceil() as usize;
        if window.len() <= tau_max + 2 {
            return None;
        }
        let w = window.len() - tau_max - 1;
        self.difference(window, w, tau_max + 1);
        // Normalise by the cumulative mean
        let mut running = 0.0f32;
        self.diff[0] = 1.0;
        for tau in 1..=tau_max + 1 {
            running += self.diff[tau];
            self.diff[tau] = if running > 0.0 { self.diff[tau] * tau as f32 / running } else { 1.0 };
        }
        let mut tau = tau_min.max(2);
        let mut found = None;
        while tau <= tau_max {
            if self.diff[tau] < self.threshold {
                while tau < tau_max && self.diff[tau + 1] < self.diff[tau] {
                    tau += 1;
                }
                found = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = found?;
        // Parabolic interpolation
        let (a, b, c) = (self.diff[tau - 1], self.diff[tau], self.diff[tau + 1]);
        let denom = a - 2.0 * b + c;
        let shift = if denom.abs() > 1e-12 { 0.5 * (a - c) / denom } else { 0.0 };
        Frequency::new(fs / (tau as f32 + shift)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harmonic_tone(f0: f32, fs: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (1..=10).map(|k| (2.0 * std::f32::consts::PI * k as f32 * f0 * i as f32 / fs).sin() / k as f32).sum())
            .collect()
    }

    #[test]
    fn finds_f0_within_a_tenth_of_a_percent() {
        let mut yin = Yin::default();
        let rate = SampleRate::new(48_000).unwrap();
        for f0 in [80.0, 100.0, 150.0, 220.0, 440.0] {
            let x = harmonic_tone(f0, 48_000.0, 4096);
            let est = yin.estimate(&x, rate).expect("voiced").hz();
            assert!((est / f0 - 1.0).abs() < 0.001, "{f0}: {est}");
        }
    }

    #[test]
    fn silence_is_unvoiced() {
        let mut yin = Yin::default();
        assert!(yin.estimate(&vec![0.0; 4096], SampleRate::new(48_000).unwrap()).is_none());
    }
}
