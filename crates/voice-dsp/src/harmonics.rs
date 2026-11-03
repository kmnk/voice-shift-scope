//! Picks harmonics from the FFT spectrum and counts the harmonics seen.
//!
//! The noise floor for each harmonic is the median amplitude between it and the neighbouring harmonics (excluding the window's main lobe).
//! Measuring it in parts without voice was not used, because it cannot be updated while the voice continues.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use voice_domain::ports::HarmonicPicker;
use voice_domain::{AnalysisSettings, Decibels, Frequency, Harmonic, SampleRate};

/// Allowed distance from k × F0 when searching for a harmonic (as a ratio of F0). Within ±5% of F0.
pub const PEAK_TOLERANCE: f32 = 0.05;
/// Maximum number of harmonics to pick.
pub const MAX_HARMONICS: usize = 40;

pub struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    size: usize,
    buf: Vec<Complex32>,
    /// The latest amplitudes (dB, a full-scale sine is 0 dB). Length size/2+1.
    pub db: Vec<f32>,
    /// Window length (samples). Sets the width of the main lobe.
    pub window_len: usize,
}

impl Spectrum {
    pub fn new(size: usize) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(size);
        Self { fft, size, buf: vec![Complex32::default(); size], db: vec![0.0; size / 2 + 1], window_len: 0 }
    }

    /// Applies a Hann window, pads with zeros up to `size`, and transforms.
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
    /// Refines F0 from the harmonic peaks near `f0` (least squares: f0 = Σ k p_k / Σ k²).
    fn refine_f0(&self, fs: f32, f0: f32, upto: f32) -> f32 {
        let bin = self.spectrum.bin_hz(fs);
        let (mut num, mut den) = (0.0f32, 0.0f32);
        let mut k = 1;
        while (k as f32) * f0 < upto.min(fs / 2.0) && k <= 10 {
            // Use only peaks well above the noise floor (so peaks where there is no harmonic do not move F0)
            if let Some((p, level)) = self.peak(k as f32 * f0, f0 / 4.0, bin)
                && level - self.floor(k as f32 * f0, f0, fs) > 20.0
            {
                num += k as f32 * p;
                den += (k * k) as f32;
            }
            k += 1;
        }
        if den > 0.0 { num / den } else { f0 }
    }

    /// Frequency (interpolated with a parabola) and level of the largest peak within `center` ± `half` Hz.
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

    /// The noise floor of harmonic k: the median amplitude within F0/2 of k×F0, excluding the main lobe.
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
        for k in 1..=MAX_HARMONICS {
            let target = k as f32 * f0;
            if target >= upto.min(fs / 2.0) {
                break;
            }
            let Some((freq, level)) = self.peak(target, f0 / 4.0, bin) else { break };
            let floor = self.floor(target, f0, fs);
            let observed =
                level - floor > settings.threshold.db() && (freq - target).abs() <= PEAK_TOLERANCE * f0;
            if let (Ok(frequency), Ok(level)) = (Frequency::new(freq), Decibels::new(level)) {
                harmonics.push(Harmonic { number: k as u32, frequency, level, observed });
            }
        }
        // The second condition: do not count harmonics weaker than the limit below the strongest harmonic seen
        let strongest = harmonics.iter().filter(|h| h.observed).map(|h| h.level.db()).fold(f32::NEG_INFINITY, f32::max);
        for h in harmonics.iter_mut() {
            if h.level.db() < strongest - settings.relative_limit.db() {
                h.observed = false;
            }
        }
        let observed_count = harmonics.iter().take_while(|h| h.observed).count() as u32;
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

    /// For a voice whose higher harmonics are weaker (−24 dB per octave, a falsetto-like slope), harmonics more than 40 dB below the strongest are not counted.
    /// For a voice with a gentler slope (−12 dB/oct), many more are counted.
    #[test]
    fn relative_limit_counts_fewer_harmonics_for_steep_spectra() {
        let rate = SampleRate::new(48_000).unwrap();
        let tone = |slope_db_per_octave: f32| -> Vec<f32> {
            (0..4096)
                .map(|i| {
                    (1..=30)
                        .map(|k| {
                            let a = 10f32.powf(slope_db_per_octave * (k as f32).log2() / 20.0);
                            0.1 * a * (2.0 * std::f32::consts::PI * k as f32 * 150.0 * i as f32 / 48_000.0).cos()
                        })
                        .sum()
                })
                .collect()
        };
        let mut picker = SpectralHarmonics::default();
        let settings = AnalysisSettings::default();
        let (_, steep) = picker.pick(&tone(-24.0), rate, Frequency::new(150.0).unwrap(), &settings);
        let (_, gentle) = picker.pick(&tone(-12.0), rate, Frequency::new(150.0).unwrap(), &settings);
        // A 40 dB difference is 2^(40/24) ≈ 3.2 times the frequency at −24 dB/oct, and 2^(40/12) ≈ 10 times at −12 dB/oct
        assert_eq!(steep, 3, "steep");
        assert_eq!(gentle, 10, "gentle");
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
