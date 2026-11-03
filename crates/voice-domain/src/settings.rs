//! Setting values the user changes on screen. The ranges are provisional and settled during implementation.

use crate::DomainError;

fn finite(value: f32) -> Result<f32, DomainError> {
    if value.is_finite() { Ok(value) } else { Err(DomainError::NotFinite) }
}

/// Pitch change (semitones). −24 to +24, which covers the minimum required range (±12 semitones).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchShift(f32);

impl PitchShift {
    pub const MIN: f32 = -24.0;
    pub const MAX: f32 = 24.0;
    pub const NONE: Self = Self(0.0);

    pub fn new(semitones: f32) -> Result<Self, DomainError> {
        let v = finite(semitones)?;
        if (Self::MIN..=Self::MAX).contains(&v) {
            Ok(Self(v))
        } else {
            Err(DomainError::InvalidPitchShift { value: v, min: Self::MIN, max: Self::MAX })
        }
    }

    pub fn semitones(self) -> f32 {
        self.0
    }

    /// Frequency ratio (2^(semitones/12)).
    pub fn ratio(self) -> f32 {
        2f32.powf(self.0 / 12.0)
    }
}

/// Formant factor. 0.5 to 2.0, which covers the minimum required range (0.7 to 1.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormantShift(f32);

impl FormantShift {
    pub const MIN: f32 = 0.5;
    pub const MAX: f32 = 2.0;
    pub const NONE: Self = Self(1.0);

    pub fn new(ratio: f32) -> Result<Self, DomainError> {
        let v = finite(ratio)?;
        if (Self::MIN..=Self::MAX).contains(&v) {
            Ok(Self(v))
        } else {
            Err(DomainError::InvalidFormantShift { value: v, min: Self::MIN, max: Self::MAX })
        }
    }

    pub fn ratio(self) -> f32 {
        self.0
    }
}

/// Settings for changing the voice. Pitch and formant change independently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShiftSettings {
    pub pitch: PitchShift,
    pub formant: FormantShift,
}

impl ShiftSettings {
    pub const NONE: Self = Self { pitch: PitchShift::NONE, formant: FormantShift::NONE };
}

impl Default for ShiftSettings {
    fn default() -> Self {
        Self::NONE
    }
}

/// Upper limit of the formant estimate (Hz). Defaults to 5500, as in Praat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormantCeiling(f32);

impl FormantCeiling {
    pub const MIN: f32 = 3000.0;
    pub const MAX: f32 = 8000.0;
    pub const DEFAULT: Self = Self(5500.0);

    pub fn new(hz: f32) -> Result<Self, DomainError> {
        let v = finite(hz)?;
        if (Self::MIN..=Self::MAX).contains(&v) {
            Ok(Self(v))
        } else {
            Err(DomainError::InvalidFormantCeiling { value: v, min: Self::MIN, max: Self::MAX })
        }
    }

    pub fn hz(self) -> f32 {
        self.0
    }
}

/// How far above the noise floor a harmonic must be to count as seen (dB). The default of 10 is provisional.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HarmonicThreshold(f32);

impl HarmonicThreshold {
    pub const MIN: f32 = 0.0;
    pub const MAX: f32 = 40.0;
    pub const DEFAULT: Self = Self(10.0);

    pub fn new(db: f32) -> Result<Self, DomainError> {
        let v = finite(db)?;
        if (Self::MIN..=Self::MAX).contains(&v) {
            Ok(Self(v))
        } else {
            Err(DomainError::InvalidHarmonicThreshold { value: v, min: Self::MIN, max: Self::MAX })
        }
    }

    pub fn db(self) -> f32 {
        self.0
    }
}

/// How far below the strongest harmonic a harmonic may be to count as seen (dB). The default of 40 is provisional.
/// The noise floor alone would count weak high harmonics of a falsetto in a quiet room, so this is a second condition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HarmonicRelativeLimit(f32);

impl HarmonicRelativeLimit {
    pub const MIN: f32 = 10.0;
    pub const MAX: f32 = 100.0;
    pub const DEFAULT: Self = Self(40.0);

    pub fn new(db: f32) -> Result<Self, DomainError> {
        let v = finite(db)?;
        if (Self::MIN..=Self::MAX).contains(&v) {
            Ok(Self(v))
        } else {
            Err(DomainError::InvalidHarmonicRelativeLimit { value: v, min: Self::MIN, max: Self::MAX })
        }
    }

    pub fn db(self) -> f32 {
        self.0
    }
}

/// LPC order (number of poles). Twice the number of formants, as in Praat. Defaults to 10 (5 formants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LpcOrder(u32);

impl LpcOrder {
    pub const MIN: u32 = 4;
    pub const MAX: u32 = 20;
    pub const DEFAULT: Self = Self(10);

    pub fn new(order: u32) -> Result<Self, DomainError> {
        if (Self::MIN..=Self::MAX).contains(&order) && order.is_multiple_of(2) {
            Ok(Self(order))
        } else {
            Err(DomainError::InvalidLpcOrder { value: order, min: Self::MIN, max: Self::MAX })
        }
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// Analysis settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalysisSettings {
    pub ceiling: FormantCeiling,
    pub threshold: HarmonicThreshold,
    pub relative_limit: HarmonicRelativeLimit,
    pub lpc_order: LpcOrder,
}

impl AnalysisSettings {
    /// Settings for analysing the shifted voice. The ceiling rises as much as the formants were raised (up to the top of its range).
    /// Keeping the ceiling would push the raised upper resonances above it and place formants where there are none.
    pub fn for_shifted(self, formant: crate::FormantShift) -> Self {
        let scaled = (self.ceiling.hz() * formant.ratio()).clamp(FormantCeiling::MIN, FormantCeiling::MAX);
        Self { ceiling: FormantCeiling(scaled), ..self }
    }
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            ceiling: FormantCeiling::DEFAULT,
            threshold: HarmonicThreshold::DEFAULT,
            relative_limit: HarmonicRelativeLimit::DEFAULT,
            lpc_order: LpcOrder::DEFAULT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_shift_accepts_spec_range_and_rejects_outside() {
        assert!(PitchShift::new(-12.0).is_ok());
        assert!(PitchShift::new(12.0).is_ok());
        assert!(PitchShift::new(24.1).is_err());
        assert!(PitchShift::new(f32::NAN).is_err());
        assert!((PitchShift::new(12.0).unwrap().ratio() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn formant_shift_accepts_spec_range_and_rejects_outside() {
        assert!(FormantShift::new(0.7).is_ok());
        assert!(FormantShift::new(1.4).is_ok());
        assert!(FormantShift::new(0.49).is_err());
        assert!(FormantShift::new(f32::INFINITY).is_err());
    }

    #[test]
    fn shifted_analysis_raises_ceiling_with_formant_ratio() {
        let base = AnalysisSettings::default();
        assert!((base.for_shifted(FormantShift::new(1.2).unwrap()).ceiling.hz() - 6600.0).abs() < 0.01);
        assert_eq!(base.for_shifted(FormantShift::new(1.5).unwrap()).ceiling.hz(), FormantCeiling::MAX);
        assert_eq!(base.for_shifted(FormantShift::new(0.5).unwrap()).ceiling.hz(), FormantCeiling::MIN);
    }

    #[test]
    fn analysis_values_check_their_ranges() {
        assert!(FormantCeiling::new(2999.0).is_err());
        assert!(HarmonicThreshold::new(-1.0).is_err());
        assert!(LpcOrder::new(11).is_err());
        assert!(LpcOrder::new(10).is_ok());
    }
}
