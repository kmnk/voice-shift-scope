//! 3 つの推定を組み合わせて `AnalysisFrame` を作る。

use voice_domain::ports::{Analyzer, FormantEstimator, HarmonicPicker, PitchEstimator};
use voice_domain::{AnalysisFrame, AnalysisSettings, SampleRate};

use crate::harmonics::SpectralHarmonics;
use crate::lpc::BurgFormants;
use crate::yin::Yin;

/// 分析の窓の長さ（48 kHz で約 85 ms）。倍音の山を分けるのに要る長さ。
pub const ANALYSIS_WINDOW: usize = 4096;
/// 画面に送るスペクトルの点の数（0〜上限）。
pub const DISPLAY_POINTS: usize = 256;

#[derive(Default)]
pub struct DefaultAnalyzer {
    pub pitch: Yin,
    pub formants: BurgFormants,
    pub harmonics: SpectralHarmonics,
}

impl Analyzer for DefaultAnalyzer {
    fn analyze(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> AnalysisFrame {
        let f0 = self.pitch.estimate(window, rate);
        let mut frame = AnalysisFrame { f0, voiced: f0.is_some(), ..Default::default() };
        if let Some(f0) = f0 {
            frame.formants = self.formants.estimate(window, rate, settings);
            let (harmonics, observed) = self.harmonics.pick(window, rate, f0, settings);
            frame.harmonics = harmonics;
            frame.observed_harmonics = observed;
        } else {
            self.harmonics.spectrum.compute(window);
        }
        frame.spectrum = display_spectrum(&self.harmonics, rate, settings);
        frame
    }

    fn pitch_only(&mut self, window: &[f32], rate: SampleRate, _settings: &AnalysisSettings) -> Option<voice_domain::Frequency> {
        self.pitch.estimate(window, rate)
    }
}

/// 0〜上限を `DISPLAY_POINTS` に分け、各区間の最大の振幅（dB）を取る。
fn display_spectrum(h: &SpectralHarmonics, rate: SampleRate, settings: &AnalysisSettings) -> Vec<f32> {
    let bin = h.spectrum.bin_hz(rate.hz() as f32);
    let per_point = settings.ceiling.hz() / DISPLAY_POINTS as f32;
    (0..DISPLAY_POINTS)
        .map(|p| {
            let lo = (p as f32 * per_point / bin) as usize;
            let hi = (((p + 1) as f32 * per_point / bin) as usize).max(lo + 1).min(h.spectrum.db.len());
            h.spectrum.db[lo.min(hi - 1)..hi].iter().copied().fold(f32::NEG_INFINITY, f32::max)
        })
        .collect()
}
