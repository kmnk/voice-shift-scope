//! YIN による F0 の推定（de Cheveigné & Kawahara 2002）。

use voice_domain::ports::PitchEstimator;
use voice_domain::{Frequency, SampleRate};

pub struct Yin {
    pub min_f0: f32,
    pub max_f0: f32,
    /// 累積平均で正規化した差の関数のしきい値。
    pub threshold: f32,
    /// これより小さい RMS（dBFS）は無声とする。
    pub silence_dbfs: f32,
    diff: Vec<f32>,
}

impl Default for Yin {
    fn default() -> Self {
        Self { min_f0: 40.0, max_f0: 1000.0, threshold: 0.15, silence_dbfs: -55.0, diff: Vec::new() }
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
        self.diff.clear();
        self.diff.resize(tau_max + 2, 0.0);
        for tau in 1..=tau_max + 1 {
            let mut acc = 0.0f32;
            for j in 0..w {
                let d = window[j] - window[j + tau];
                acc += d * d;
            }
            self.diff[tau] = acc;
        }
        // 累積平均で正規化
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
        // 放物線で補間
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
            let est = yin.estimate(&x, rate).expect("有声").hz();
            assert!((est / f0 - 1.0).abs() < 0.001, "{f0}: {est}");
        }
    }

    #[test]
    fn silence_is_unvoiced() {
        let mut yin = Yin::default();
        assert!(yin.estimate(&vec![0.0; 4096], SampleRate::new(48_000).unwrap()).is_none());
    }
}
