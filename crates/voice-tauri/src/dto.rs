//! 画面に送る形。domain の型は画面の形を知らないので、ここで直す。

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
    pub monitors: Vec<DeviceDto>,
    /// 見つかった VB-CABLE の再生側と録音側の名前。見つからなければ null。
    pub cable: Option<CableDto>,
    pub selected_input: Option<String>,
    pub selected_monitor: Option<String>,
    pub monitor_enabled: bool,
    pub monitor_volume: f32,
    pub running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CableDto {
    pub playback: String,
    pub recording: String,
}

pub fn devices_dto(list: &DeviceList, session: &VoiceSession, names: impl Fn(&str) -> String) -> DevicesDto {
    DevicesDto {
        inputs: list.inputs().map(DeviceDto::from).collect(),
        monitors: list.monitors().map(DeviceDto::from).collect(),
        cable: list.cable().map(|c| CableDto { playback: names(c.playback.raw()), recording: names(c.recording.raw()) }),
        selected_input: session.input().map(|d| d.raw().to_string()),
        selected_monitor: session.monitor().device.as_ref().map(|d| d.raw().to_string()),
        monitor_enabled: session.monitor().enabled,
        monitor_volume: session.monitor().volume.get(),
        running: session.state() == voice_domain::SessionState::Running,
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
    /// H1〜H10 まで。
    pub harmonics: Vec<HarmonicDto>,
    pub observed_harmonics: u32,
    /// 0〜上限を 256 に分けた振幅（dB、小数 1 桁に丸める）。
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
                .take(10)
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
    pub ceiling: f32,
    pub threshold: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PairDto {
    /// 入力を始めてからの秒数。
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
                threshold: p.analysis.threshold.db(),
            },
        }
    }
}
