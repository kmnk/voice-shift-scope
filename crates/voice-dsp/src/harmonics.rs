//! FFT のスペクトルから倍音を拾い、観測できた倍音の数を数える。
//!
//! 雑音の床は、倍音ごとに、隣の倍音との間（窓の主葉を除く）の振幅の中央値とする。
//! 声の無い区間で測る形は、声を出し続けている間は更新できないので採らなかった。

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use voice_domain::ports::HarmonicPicker;
use voice_domain::{AnalysisSettings, Decibels, Frequency, Harmonic, SampleRate};

/// 倍音を探すときの、k × F0 からの許す幅（F0 に対する比）。spec の「F0 の ±5% 以内」。
pub const PEAK_TOLERANCE: f32 = 0.05;
/// 拾う倍音の最大の数。
pub const MAX_HARMONICS: usize = 40;

pub struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    size: usize,
    buf: Vec<Complex32>,
    /// 直近の振幅（dB、full scale の sine が 0 dB）。長さ size/2+1。
    pub db: Vec<f32>,
    /// 窓の長さ（標本）。主葉の幅を決める。
    pub window_len: usize,
}

impl Spectrum {
    pub fn new(size: usize) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(size);
        Self { fft, size, buf: vec![Complex32::default(); size], db: vec![0.0; size / 2 + 1], window_len: 0 }
    }

    /// Hann の窓を掛けて、`size` まで 0 で埋めて変換する。
    pub fn compute(&mut self, x: &[f32]) {
        let n = x.len().min(self.size);
        self.window_len = n;
        let mut wsum = 0.0f32;
        for (i, slot) in self.buf.iter_mut().enumerate() {
            *slot = if i < n {
                let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos();
                wsum += w;
                Complex32::new(x[i] * w, 0.0)
            } else {
                Complex32::default()
            };
        }
        self.fft.process(&mut self.buf);
        for (d, c) in self.db.iter_mut().zip(&self.buf) {
            *d = 20.0 * (2.0 * c.norm() / wsum).max(1e-10).log10();
        }
    }

    pub fn bin_hz(&self, fs: f32) -> f32 {
        fs / self.size as f32
    }
}

pub struct SpectralHarmonics {
    pub spectrum: Spectrum,
}

impl Default for SpectralHarmonics {
    fn default() -> Self {
        Self { spectrum: Spectrum::new(16_384) }
    }
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return f32::NEG_INFINITY;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

impl SpectralHarmonics {
    /// `f0` の近くの倍音の山から F0 を詰め直す（最小二乗で f0 = Σ k p_k / Σ k²）。
    fn refine_f0(&self, fs: f32, f0: f32, upto: f32) -> f32 {
        let bin = self.spectrum.bin_hz(fs);
        let (mut num, mut den) = (0.0f32, 0.0f32);
        let mut k = 1;
        while (k as f32) * f0 < upto.min(fs / 2.0) && k <= 10 {
            // 雑音の床より十分に高い山だけを使う（無い倍音の位置の山で F0 がずれないように）
            if let Some((p, level)) = self.peak(k as f32 * f0, f0 / 4.0, bin) {
                if level - self.floor(k as f32 * f0, f0, fs) > 20.0 {
                    num += k as f32 * p;
                    den += (k * k) as f32;
                }
            }
            k += 1;
        }
        if den > 0.0 { num / den } else { f0 }
    }

    /// `center` ± `half` Hz の中の最大の山の周波数（放物線で補間）と強さ。
    fn peak(&self, center: f32, half: f32, bin: f32) -> Option<(f32, f32)> {
        let db = &self.spectrum.db;
        let lo = ((center - half) / bin).floor().max(1.0) as usize;
        let hi = (((center + half) / bin).ceil() as usize).min(db.len() - 2);
        if lo >= hi {
            return None;
        }
        let i = (lo..=hi).max_by(|a, b| db[*a].total_cmp(&db[*b]))?;
        let (a, b, c) = (db[i - 1], db[i], db[i + 1]);
        let denom = a - 2.0 * b + c;
        let shift = if denom.abs() > 1e-9 { 0.5 * (a - c) / denom } else { 0.0 };
        Some(((i as f32 + shift) * bin, b - 0.25 * (a - c) * shift))
    }

    /// 倍音 k の雑音の床: k×F0 の前後 F0/2 のうち、主葉を除いた所の振幅の中央値。
    fn floor(&self, center: f32, f0: f32, fs: f32) -> f32 {
        let bin = self.spectrum.bin_hz(fs);
        let lobe = 2.5 * fs / self.spectrum.window_len.max(1) as f32;
        let db = &self.spectrum.db;
        let lo = ((center - f0 / 2.0) / bin).ceil().max(0.0) as usize;
        let hi = (((center + f0 / 2.0) / bin).floor() as usize).min(db.len() - 1);
        let mut v: Vec<f32> = (lo..=hi)
            .filter(|i| ((*i as f32) * bin - center).abs() > lobe)
            .map(|i| db[i])
            .collect();
        median(&mut v)
    }
}

impl HarmonicPicker for SpectralHarmonics {
    fn pick(
        &mut self,
        window: &[f32],
        rate: SampleRate,
        f0: Frequency,
        settings: &AnalysisSettings,
    ) -> (Vec<Harmonic>, u32) {
        let fs = rate.hz() as f32;
        self.spectrum.compute(window);
        let upto = settings.ceiling.hz();
        let f0 = self.refine_f0(fs, f0.hz(), upto);
        let bin = self.spectrum.bin_hz(fs);
        let mut harmonics = Vec::new();
        let mut observed_count = 0u32;
        let mut still_counting = true;
        for k in 1..=MAX_HARMONICS {
            let target = k as f32 * f0;
            if target >= upto.min(fs / 2.0) {
                break;
            }
            let Some((freq, level)) = self.peak(target, f0 / 4.0, bin) else { break };
            let floor = self.floor(target, f0, fs);
            let observed =
                level - floor > settings.threshold.db() && (freq - target).abs() <= PEAK_TOLERANCE * f0;
            if still_counting && observed {
                observed_count += 1;
            } else {
                still_counting = false;
            }
            if let (Ok(frequency), Ok(level)) = (Frequency::new(freq), Decibels::new(level)) {
                harmonics.push(Harmonic { number: k as u32, frequency, level, observed });
            }
        }
        (harmonics, observed_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partials(f0: f32, count: usize, fs: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (1..=count).map(|k| 0.1 * (2.0 * std::f32::consts::PI * k as f32 * f0 * i as f32 / fs).cos()).sum())
            .collect()
    }

    #[test]
    fn counts_eight_partials_and_reports_their_frequencies() {
        let rate = SampleRate::new(48_000).unwrap();
        let x = partials(150.0, 8, 48_000.0, 4096);
        let mut picker = SpectralHarmonics::default();
        let (h, count) = picker.pick(&x, rate, Frequency::new(151.0).unwrap(), &AnalysisSettings::default());
        assert_eq!(count, 8);
        for hk in h.iter().take(8) {
            let expected = 150.0 * hk.number as f32;
            assert!((hk.frequency.hz() / expected - 1.0).abs() < 0.01, "{hk:?}");
        }
    }
}
