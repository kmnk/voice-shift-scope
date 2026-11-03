use thiserror::Error;

/// Errors from value checks and the rules of `VoiceSession`.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DomainError {
    #[error("pitch must be {min} to {max} semitones ({value})")]
    InvalidPitchShift { value: f32, min: f32, max: f32 },
    #[error("formant factor must be {min} to {max} ({value})")]
    InvalidFormantShift { value: f32, min: f32, max: f32 },
    #[error("formant ceiling must be {min} to {max} Hz ({value})")]
    InvalidFormantCeiling { value: f32, min: f32, max: f32 },
    #[error("harmonics floor must be {min} to {max} dB ({value})")]
    InvalidHarmonicThreshold { value: f32, min: f32, max: f32 },
    #[error("harmonics range below the strongest must be {min} to {max} dB ({value})")]
    InvalidHarmonicRelativeLimit { value: f32, min: f32, max: f32 },
    #[error("LPC order must be an even number from {min} to {max} ({value})")]
    InvalidLpcOrder { value: u32, min: u32, max: u32 },
    #[error("monitor volume must be 0 to 1 ({value})")]
    InvalidMonitorVolume { value: f32 },
    #[error("the value is not a number")]
    NotFinite,
    #[error("the sample rate must be greater than 0")]
    InvalidSampleRate,
    #[error("the frequency must be greater than 0 ({value})")]
    InvalidFrequency { value: f32 },
    #[error("the device is not in the list ({0})")]
    UnknownDevice(String),
    #[error("the recording side of the output (VB-CABLE) cannot be the input")]
    CableRecordingAsInput,
    #[error("the monitor cannot be the same device as the output")]
    MonitorSameAsOutput,
    #[error("choose an output device")]
    NoOutputDevice,
    #[error("the output device cannot be changed while running")]
    OutputChangeWhileRunning,
    #[error("cannot start without an input device")]
    NoInputDevice,
    #[error("choose a monitor device to turn the monitor on")]
    NoMonitorDevice,
    #[error("the input device cannot be changed while running")]
    InputChangeWhileRunning,
    #[error("already running")]
    AlreadyRunning,
    #[error("not running")]
    NotRunning,
}
