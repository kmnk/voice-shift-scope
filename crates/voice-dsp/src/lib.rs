//! 信号処理の adapter: 分析の推定（YIN、Burg 法の LPC、倍音の拾い出し）と、
//! 声を変える処理の実装。domain の port を実装する。


pub mod analyzer;
pub mod harmonics;
pub mod lpc;
pub mod resample;
pub mod shifter;
pub mod yin;

pub use analyzer::{ANALYSIS_WINDOW, DefaultAnalyzer};
pub use shifter::{DEFAULT_BLOCK_SECONDS, FORMANT_BASE_RATIO, SignalsmithShifter};
