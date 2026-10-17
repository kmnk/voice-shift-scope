//! 分析の結果の値。

use crate::{AnalysisSettings, DomainError, ShiftSettings};

/// 標本化の速さ（Hz）。
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

/// 入力を始めてからの標本の数。時刻はこれで表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SamplePosition(pub u64);

/// 周波数（Hz、0 より大きい）。
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

/// 強さ（dB）。
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

/// 1 つのフォルマント。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Formant {
    pub frequency: Frequency,
    pub bandwidth: Frequency,
}

/// 1 本の倍音。`number` は 1 が基本周波数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Harmonic {
    pub number: u32,
    pub frequency: Frequency,
    pub level: Decibels,
    /// 雑音の床からしきい値を超えて、k × F0 の近くに山がある。
    pub observed: bool,
}

/// 1 つの時刻の分析の結果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalysisFrame {
    pub f0: Option<Frequency>,
    pub voiced: bool,
    pub formants: [Option<Formant>; 4],
    pub harmonics: Vec<Harmonic>,
    /// 1 番目から途切れずに観測できた倍音の数。
    pub observed_harmonics: u32,
    /// 画面の描画に使う列（dB）。
    pub spectrum: Vec<f32>,
    pub envelope: Vec<f32>,
    pub noise_floor: Vec<f32>,
}

/// 同じ時刻の、変える前と変えた後の分析の結果と、その時の設定。
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisPair {
    pub position: SamplePosition,
    pub before: AnalysisFrame,
    pub after: AnalysisFrame,
    pub shift: ShiftSettings,
    pub analysis: AnalysisSettings,
}
