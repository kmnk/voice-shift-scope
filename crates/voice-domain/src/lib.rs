//! domain: value objects, `VoiceSession` (aggregate), and port traits.
//! Does not depend on outside technology (cpal, Signalsmith Stretch, Tauri).
//! Values are checked by their constructors, and fields are not reachable from outside. No serde.

mod analysis;
mod devices;
mod error;
pub mod ports;
mod session;
mod settings;

pub use analysis::{AnalysisFrame, AnalysisPair, Decibels, Formant, Frequency, Harmonic, SamplePosition, SampleRate};
pub use devices::{DeviceDirection, DeviceId, DeviceInfo, DeviceList, VirtualCable};
pub use error::DomainError;
pub use session::{AudioSource, MonitorSettings, MonitorVolume, Routing, SessionState, VoiceSession};
pub use settings::{
    AnalysisSettings, FormantCeiling, FormantShift, HarmonicRelativeLimit, HarmonicThreshold, LpcOrder, PitchShift, ShiftSettings,
};
