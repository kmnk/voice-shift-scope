//! Signal processing adapter: the analysis estimates (YIN, Burg LPC with F1–F4 tracking, harmonic picking) and
//! the voice shifters. Implements the domain ports.


pub mod analyzer;
pub mod formant_track;
pub mod harmonics;
pub mod hybrid;
pub mod level;
pub mod lpc;
pub mod psola;
pub mod resample;
pub mod shifter;
pub mod yin;

pub use analyzer::{ANALYSIS_WINDOW, DefaultAnalyzer};
pub use formant_track::{FORMANT_DELAY_FRAMES, FormantTracker};
pub use hybrid::HybridShifter;
pub use level::{LevelMatch, LevelMatched};
pub use psola::PsolaShifter;
pub use shifter::{DEFAULT_BLOCK_SECONDS, FORMANT_BASE_RATIO, SignalsmithShifter};

/// The app's shifter: PSOLA when lowering the pitch, Signalsmith Stretch otherwise,
/// with the shifted level matched to the original (`HybridShifter`). The app and the test tools use the same processing through this.
/// Allocates memory when created, so create it outside the audio thread.
pub fn app_shifter(sample_rate: u32) -> Box<dyn voice_domain::ports::VoiceShifter> {
    Box::new(app_shifter_with_block(sample_rate, DEFAULT_BLOCK_SECONDS))
}

/// `app_shifter` with a configurable Signalsmith Stretch window length (seconds). The interval is 1/4 of the window.
pub fn app_shifter_with_block(sample_rate: u32, block_seconds: f32) -> HybridShifter {
    let signalsmith = SignalsmithShifter::with_block(sample_rate, block_seconds, block_seconds / 4.0);
    HybridShifter::new(signalsmith, sample_rate, APP_MAX_BLOCK)
}

/// The largest length processed at once inside one `process` of `app_shifter`. Longer input is processed in parts.
const APP_MAX_BLOCK: usize = 1024;

#[cfg(test)]
mod app_tests {
    #[test]
    fn app_shifter_reports_the_same_latency_as_before() {
        assert_eq!(super::app_shifter(48_000).latency(), 3360);
    }
}
