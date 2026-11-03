//! Builds an `AnalysisFrame` from three estimates.

use voice_domain::ports::{Analyzer, FormantEstimator, HarmonicPicker, PitchEstimator};
use voice_domain::{AnalysisFrame, AnalysisSettings, SampleRate};

use crate::formant_track::FormantTracker;
use crate::harmonics::SpectralHarmonics;
use crate::lpc::BurgFormants;
use crate::yin::Yin;

/// Length of the analysis window (about 85 ms at 48 kHz). Long enough to separate the harmonic peaks.
pub const ANALYSIS_WINDOW: usize = 4096;
/// Number of spectrum points sent to the screen (0 to the ceiling).
pub const DISPLAY_POINTS: usize = 256;

#[derive(Default)]
pub struct DefaultAnalyzer {
    pub pitch: Yin,
    pub formants: BurgFormants,
    pub harmonics: SpectralHarmonics,
    /// F1–F4 tracking. The F1–F4 returned are from `FORMANT_DELAY_FRAMES` frames earlier (harmonics and F0 are from the current frame)
    pub tracker: FormantTracker,
    /// Do not track F1–F4; take the 4 lowest roots of each frame (for comparison during development; the app does not use it)
    pub raw_formants: bool,
}

impl Analyzer for DefaultAnalyzer {
    fn analyze(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> AnalysisFrame {
        let f0 = self.pitch.estimate(window, rate);
        let mut frame = AnalysisFrame { f0, voiced: f0.is_some(), ..Default::default() };
        if let Some(f0) = f0 {
            frame.formants = if self.raw_formants {
                self.formants.estimate(window, rate, settings)
            } else {
                let candidates = self.formants.candidates(window, rate, settings);
                self.tracker.push(&candidates, settings.ceiling.hz() as f64)
            };
            let (harmonics, observed) = self.harmonics.pick(window, rate, f0, settings);
            frame.harmonics = harmonics;
            frame.observed_harmonics = observed;
        } else {
            self.tracker.reset();
            self.harmonics.spectrum.compute(window);
        }
        frame.spectrum = display_spectrum(&self.harmonics, rate, settings);
        frame
    }

    fn pitch_only(&mut self, window: &[f32], rate: SampleRate, _settings: &AnalysisSettings) -> Option<voice_domain::Frequency> {
        self.pitch.estimate(window, rate)
    }
}

/// Splits 0 to the ceiling into `DISPLAY_POINTS` bins and takes the largest amplitude (dB) in each.
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
