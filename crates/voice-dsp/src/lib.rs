//! 信号処理の adapter: 分析の推定（YIN、Burg 法の LPC、倍音の拾い出し）と、
//! 声を変える処理の実装。domain の port を実装する。


pub mod analyzer;
pub mod harmonics;
pub mod hybrid;
pub mod level;
pub mod lpc;
pub mod psola;
pub mod resample;
pub mod shifter;
pub mod yin;

pub use analyzer::{ANALYSIS_WINDOW, DefaultAnalyzer};
pub use hybrid::HybridShifter;
pub use level::{LevelMatch, LevelMatched};
pub use psola::PsolaShifter;
pub use shifter::{DEFAULT_BLOCK_SECONDS, FORMANT_BASE_RATIO, SignalsmithShifter};

/// アプリの声を変える処理: ピッチを下げるときは PSOLA、それ以外は Signalsmith Stretch にし、
/// 変えた後の音量を変える前にそろえる（`HybridShifter`）。アプリと試験の道具は、これを通して同じ処理を使う。
/// 作るときにメモリを確保するので、音声のスレッドの外で作る。
pub fn app_shifter(sample_rate: u32) -> Box<dyn voice_domain::ports::VoiceShifter> {
    Box::new(app_shifter_with_block(sample_rate, DEFAULT_BLOCK_SECONDS))
}

/// `app_shifter` の、Signalsmith Stretch の窓の長さ（秒）を替えられるもの。間隔は窓の 1/4。
pub fn app_shifter_with_block(sample_rate: u32, block_seconds: f32) -> HybridShifter {
    let signalsmith = SignalsmithShifter::with_block(sample_rate, block_seconds, block_seconds / 4.0);
    HybridShifter::new(signalsmith, sample_rate, APP_MAX_BLOCK)
}

/// `app_shifter` の 1 回の `process` で、内側で一度に処理する最大の長さ。長い入力は分けて処理する。
const APP_MAX_BLOCK: usize = 1024;

#[cfg(test)]
mod app_tests {
    #[test]
    fn app_shifter_reports_the_same_latency_as_before() {
        assert_eq!(super::app_shifter(48_000).latency(), 3360);
    }
}
