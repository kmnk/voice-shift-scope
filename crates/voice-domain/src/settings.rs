//! 利用者が画面から変える設定の値。範囲は仮置きで、実装で詰める。

use crate::DomainError;

fn finite(value: f32) -> Result<f32, DomainError> {
    if value.is_finite() { Ok(value) } else { Err(DomainError::NotFinite) }
}

/// ピッチの変化（半音）。spec の最小の範囲（±12 半音）を含む −24〜+24。
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

    /// 周波数の比（2^(半音/12)）。
    pub fn ratio(self) -> f32 {
        2f32.powf(self.0 / 12.0)
    }
}

/// フォルマントの倍率。spec の最小の範囲（0.7〜1.4）を含む 0.5〜2.0。
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

/// 声を変える設定。ピッチとフォルマントは独立に変える。
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

/// フォルマントの推定の上限（Hz）。Praat と同じく既定 5500。
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

/// 倍音を「観測できた」とみなす、雑音の床からの差（dB）。既定 10 は仮置き。
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

/// LPC の次数（極の数）。Praat と同じく、フォルマントの数の 2 倍。既定 10（5 個）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LpcOrder(u32);

impl LpcOrder {
    pub const MIN: u32 = 4;
    pub const MAX: u32 = 20;
    pub const DEFAULT: Self = Self(10);

    pub fn new(order: u32) -> Result<Self, DomainError> {
        if (Self::MIN..=Self::MAX).contains(&order) && order % 2 == 0 {
            Ok(Self(order))
        } else {
            Err(DomainError::InvalidLpcOrder { value: order, min: Self::MIN, max: Self::MAX })
        }
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// 分析の設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalysisSettings {
    pub ceiling: FormantCeiling,
    pub threshold: HarmonicThreshold,
    pub lpc_order: LpcOrder,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            ceiling: FormantCeiling::DEFAULT,
            threshold: HarmonicThreshold::DEFAULT,
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
    fn analysis_values_check_their_ranges() {
        assert!(FormantCeiling::new(2999.0).is_err());
        assert!(HarmonicThreshold::new(-1.0).is_err());
        assert!(LpcOrder::new(11).is_err());
        assert!(LpcOrder::new(10).is_ok());
    }
}
