//! Values of analysis results.

use crate::{AnalysisSettings, DomainError, ShiftSettings};

/// Sample rate (Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleRate(u32);

impl SampleRate {
    pub fn new(hz: u32) -> Result<Self, DomainError> {
        if hz > 0 { Ok(Self(hz)) } else { Err(DomainError::InvalidSampleRate) }
    }

    pub fn hz(self) -> u32 {
        self.0
    }
}

/// Number of samples since the input started. Times are expressed with this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SamplePosition(pub u64);

/// Frequency (Hz, greater than 0).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Frequency(f32);

impl Frequency {
    pub fn new(hz: f32) -> Result<Self, DomainError> {
        if hz.is_finite() && hz > 0.0 { Ok(Self(hz)) } else { Err(DomainError::InvalidFrequency { value: hz }) }
    }

    pub fn hz(self) -> f32 {
        self.0
    }
}

/// Level (dB).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Decibels(f32);

impl Decibels {
    pub fn new(db: f32) -> Result<Self, DomainError> {
        if db.is_finite() { Ok(Self(db)) } else { Err(DomainError::NotFinite) }
    }

    pub fn db(self) -> f32 {
        self.0
    }
}

/// One formant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Formant {
    pub frequency: Frequency,
    pub bandwidth: Frequency,
}

/// One harmonic. `number` 1 is the fundamental.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Harmonic {
    pub number: u32,
    pub frequency: Frequency,
    pub level: Decibels,
    /// There is a peak near k × F0 that rises above the noise floor by more than the threshold.
    pub observed: bool,
}

/// The analysis result at one time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalysisFrame {
    pub f0: Option<Frequency>,
    pub voiced: bool,
    pub formants: [Option<Formant>; 4],
    pub harmonics: Vec<Harmonic>,
    /// Number of harmonics seen without a gap from the first.
    pub observed_harmonics: u32,
    /// Columns used to draw the screen (dB).
    pub spectrum: Vec<f32>,
    pub envelope: Vec<f32>,
    pub noise_floor: Vec<f32>,
}

/// The original and shifted analysis results at the same time, with the settings at that time.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisPair {
    pub position: SamplePosition,
    pub before: AnalysisFrame,
    pub after: AnalysisFrame,
    pub shift: ShiftSettings,
    pub analysis: AnalysisSettings,
}
