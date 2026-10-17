//! domain: value object、`VoiceSession`（aggregate）、port の trait。
//! 外の技術（cpal、Signalsmith Stretch、Tauri）に依らない。
//! 値は生成の関数で検査してから作り、field は外から触れない。serde は付けない。

mod analysis;
mod devices;
mod error;
pub mod ports;
mod session;
mod settings;

pub use analysis::{AnalysisFrame, AnalysisPair, Decibels, Formant, Frequency, Harmonic, SamplePosition, SampleRate};
pub use devices::{DeviceDirection, DeviceId, DeviceInfo, DeviceList, VirtualCable};
pub use error::DomainError;
pub use session::{MonitorSettings, MonitorVolume, SessionState, VoiceSession};
pub use settings::{
    AnalysisSettings, FormantCeiling, FormantShift, HarmonicThreshold, LpcOrder, PitchShift, ShiftSettings,
};
