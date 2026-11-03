//! The ports through which the domain talks to outside technology. Adapter crates implement them.

use crate::{
    AnalysisFrame, AnalysisPair, AnalysisSettings, DeviceId, DeviceList, Formant, Frequency, Harmonic,
    MonitorSettings, Routing, SampleRate, ShiftSettings,
};

/// Changes the voice. Called from the audio thread, so `process` must not allocate memory.
pub trait VoiceShifter: Send {
    fn set(&mut self, settings: ShiftSettings);
    fn process(&mut self, input: &[f32], output: &mut [f32]);
    /// Delay from input to output (in samples).
    fn latency(&self) -> usize;
    /// A hint of the input F0 (Hz). Only methods that use it to find the formant envelope use it.
    /// Called from the audio thread, so it must not allocate memory.
    fn set_pitch_hint(&mut self, _f0: f32) {}
}

pub trait PitchEstimator: Send {
    fn estimate(&mut self, window: &[f32], rate: SampleRate) -> Option<Frequency>;
}

pub trait FormantEstimator: Send {
    fn estimate(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> [Option<Formant>; 4];
}

pub trait HarmonicPicker: Send {
    /// Harmonics, and the number of harmonics seen without a gap from the first.
    fn pick(
        &mut self,
        window: &[f32],
        rate: SampleRate,
        f0: Frequency,
        settings: &AnalysisSettings,
    ) -> (Vec<Harmonic>, u32);
}

/// Makes an `AnalysisFrame` from the samples in a window (a domain service port).
pub trait Analyzer: Send {
    fn analyze(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> AnalysisFrame;
    /// Finds only F0 (for the hint given to the shifter while the display is off).
    fn pitch_only(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> Option<Frequency> {
        self.analyze(window, rate, settings).f0
    }
}

/// The device list. Each call fetches the current list again.
pub trait AudioDevices {
    type Error: std::error::Error + Send + Sync + 'static;
    fn list(&self) -> Result<DeviceList, Self::Error>;
}

/// Settings for starting the audio thread and the analysis thread.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    pub input: DeviceId,
    pub output: DeviceId,
    pub monitor: MonitorSettings,
    pub routing: Routing,
    pub shift: ShiftSettings,
    pub analysis: AnalysisSettings,
}

/// Starts and stops the audio thread and the analysis thread, and takes changes while they run.
pub trait AudioPipeline {
    type Error: std::error::Error + Send + Sync + 'static;
    fn start(&mut self, config: PipelineConfig) -> Result<(), Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;
    fn set_shift(&mut self, shift: ShiftSettings) -> Result<(), Self::Error>;
    fn set_analysis(&mut self, analysis: AnalysisSettings) -> Result<(), Self::Error>;
    fn set_monitor(&mut self, monitor: MonitorSettings) -> Result<(), Self::Error>;
    fn set_routing(&mut self, routing: Routing) -> Result<(), Self::Error>;
    /// Whether to run the analysis for display (formants, harmonics, sending results). F0 hints are still found while it is off.
    fn set_analysis_display(&mut self, active: bool) -> Result<(), Self::Error>;
}

/// Where analysis results go.
pub trait AnalysisPublisher: Send {
    fn publish(&self, pair: &AnalysisPair);
}

/// Where the original and shifted audio go. Only the analysis uses it now.
pub trait AudioTap: Send {
    fn push(&mut self, before: &[f32], after: &[f32]);
}
