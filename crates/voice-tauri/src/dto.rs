//! Shapes sent to the screen. Domain types do not know the screen's shapes, so they are converted here.

use serde::Serialize;
use voice_domain::{AnalysisFrame, AnalysisPair, DeviceInfo, DeviceList, VoiceSession};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

impl From<&DeviceInfo> for DeviceDto {
    fn from(d: &DeviceInfo) -> Self {
        Self { id: d.raw_id.clone(), name: d.name.clone(), is_default: d.is_default }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesDto {
    pub inputs: Vec<DeviceDto>,
    pub outputs: Vec<DeviceDto>,
    /// The monitor list, without the output device
    pub monitors: Vec<DeviceDto>,
    /// Names of the VB-CABLE playback and recording sides found. null if not found.
    pub cable: Option<CableDto>,
    pub selected_input: Option<String>,
    pub selected_output: Option<String>,
    pub selected_monitor: Option<String>,
    pub monitor_enabled: bool,
    pub monitor_volume: f32,
    /// The audio sent to the output and monitor ("shifted" or "original")
    pub output_source: &'static str,
    pub monitor_source: &'static str,
    pub running: bool,
    pub analysis: AnalysisDto,
    /// The current pitch (semitones) and formant (factor). At start these are the remembered values
    pub shift: ShiftDto,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftDto {
    pub semitones: f32,
    pub formant: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisDto {
    pub ceiling: f32,
    pub threshold: f32,
    pub relative_limit: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CableDto {
    pub playback: String,
    pub recording: String,
}

fn source_name(s: voice_domain::AudioSource) -> &'static str {
    match s {
        voice_domain::AudioSource::Shifted => "shifted",
        voice_domain::AudioSource::Original => "original",
    }
}

pub fn devices_dto(list: &DeviceList, session: &VoiceSession, names: impl Fn(&str) -> String) -> DevicesDto {
    let output = session.output().map(|d| d.raw().to_string());
    let analysis = session.analysis();
    DevicesDto {
        inputs: list.inputs().map(DeviceDto::from).collect(),
        outputs: list.outputs().map(DeviceDto::from).collect(),
        monitors: list.outputs().filter(|d| Some(&d.raw_id) != output.as_ref()).map(DeviceDto::from).collect(),
        cable: list.cable().map(|c| CableDto { playback: names(c.playback.raw()), recording: names(c.recording.raw()) }),
        selected_input: session.input().map(|d| d.raw().to_string()),
        selected_output: output.clone(),
        selected_monitor: session.monitor().device.as_ref().map(|d| d.raw().to_string()),
        monitor_enabled: session.monitor().enabled,
        monitor_volume: session.monitor().volume.get(),
        output_source: source_name(session.routing().output),
        monitor_source: source_name(session.routing().monitor),
        running: session.state() == voice_domain::SessionState::Running,
        analysis: AnalysisDto {
            ceiling: analysis.ceiling.hz(),
            threshold: analysis.threshold.db(),
            relative_limit: analysis.relative_limit.db(),
        },
        shift: ShiftDto { semitones: session.shift().pitch.semitones(), formant: session.shift().formant.ratio() },
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FormantDto {
    pub frequency: f32,
    pub bandwidth: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HarmonicDto {
    pub number: u32,
    pub frequency: f32,
    pub level: f32,
    pub observed: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FrameDto {
    pub f0: Option<f32>,
    pub voiced: bool,
    pub formants: Vec<Option<FormantDto>>,
    /// All harmonics up to the ceiling (drawn as dots in the scrolling plots; the values table shows only H1–H10).
    pub harmonics: Vec<HarmonicDto>,
    pub observed_harmonics: u32,
    /// Amplitudes of 0 to the ceiling split into 256 bins (dB, rounded to 1 decimal place).
    pub spectrum: Vec<f32>,
}

impl From<&AnalysisFrame> for FrameDto {
    fn from(f: &AnalysisFrame) -> Self {
        Self {
            f0: f.f0.map(|v| v.hz()),
            voiced: f.voiced,
            formants: f
                .formants
                .iter()
                .map(|fm| fm.map(|x| FormantDto { frequency: x.frequency.hz(), bandwidth: x.bandwidth.hz() }))
                .collect(),
            harmonics: f
                .harmonics
                .iter()
                .map(|h| HarmonicDto { number: h.number, frequency: h.frequency.hz(), level: h.level.db(), observed: h.observed })
                .collect(),
            observed_harmonics: f.observed_harmonics,
            spectrum: f.spectrum.iter().map(|v| (v.max(-140.0) * 10.0).round() / 10.0).collect(),
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub semitones: f32,
    pub formant: f32,
    /// Ceiling of the original analysis (Hz). The original spectrum covers 0 to this value.
    pub ceiling: f32,
    /// Ceiling of the shifted analysis (Hz), raised by the formant factor. The shifted spectrum covers 0 to this value.
    pub after_ceiling: f32,
    pub threshold: f32,
    pub relative_limit: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PairDto {
    /// Seconds since the input started.
    pub time: f64,
    pub before: FrameDto,
    pub after: FrameDto,
    pub settings: SettingsDto,
}

impl From<&AnalysisPair> for PairDto {
    fn from(p: &AnalysisPair) -> Self {
        Self {
            time: p.position.0 as f64 / 48_000.0,
            before: (&p.before).into(),
            after: (&p.after).into(),
            settings: SettingsDto {
                semitones: p.shift.pitch.semitones(),
                formant: p.shift.formant.ratio(),
                ceiling: p.analysis.ceiling.hz(),
                after_ceiling: p.analysis.for_shifted(p.shift.formant).ceiling.hz(),
                threshold: p.analysis.threshold.db(),
                relative_limit: p.analysis.relative_limit.db(),
            },
        }
    }
}
