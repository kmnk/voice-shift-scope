//! 1 回の「マイクから入れて仮想マイクに出す」動作。

use crate::{AnalysisSettings, DeviceDirection, DeviceId, DeviceList, DomainError, ShiftSettings, VirtualCable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Stopped,
    Running,
}

/// モニターの音量（0〜1）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorVolume(f32);

impl MonitorVolume {
    pub const DEFAULT: Self = Self(1.0);

    pub fn new(volume: f32) -> Result<Self, DomainError> {
        if volume.is_finite() && (0.0..=1.0).contains(&volume) {
            Ok(Self(volume))
        } else {
            Err(DomainError::InvalidMonitorVolume { value: volume })
        }
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// モニターの設定。起動したときは切ってある（spec の「モニターの既定」）。
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorSettings {
    pub enabled: bool,
    pub device: Option<DeviceId>,
    pub volume: MonitorVolume,
}

/// 出力やモニターに流す音。変える前の音は、変えた後と同じだけ遅らせて流す（切り替えても時刻が跳ばない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioSource {
    #[default]
    Shifted,
    Original,
}

/// 出力（仮想マイク）とモニターに、それぞれどちらの音を流すか。起動したときはどちらも変えた後。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Routing {
    pub output: AudioSource,
    pub monitor: AudioSource,
}

/// aggregate。use case はこのメソッドを通してだけ状態を変える。
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSession {
    state: SessionState,
    input: Option<DeviceId>,
    output: Option<VirtualCable>,
    monitor: MonitorSettings,
    routing: Routing,
    shift: ShiftSettings,
    analysis: AnalysisSettings,
}

impl VoiceSession {
    /// 起動したときの状態。入力とモニターは Windows の既定の機器（VB-CABLE の端なら空）、
    /// 出力の先は見つかった VB-CABLE。
    pub fn new(devices: &DeviceList) -> Self {
        Self {
            state: SessionState::Stopped,
            input: devices.default_device(DeviceDirection::Input),
            output: devices.cable().cloned(),
            monitor: MonitorSettings {
                enabled: false,
                device: devices.default_device(DeviceDirection::Output),
                volume: MonitorVolume::DEFAULT,
            },
            routing: Routing::default(),
            shift: ShiftSettings::default(),
            analysis: AnalysisSettings::default(),
        }
    }

    pub fn routing(&self) -> Routing {
        self.routing
    }

    /// 流す音の切り替えは、動いている間でもできる。
    pub fn set_routing(&mut self, routing: Routing) {
        self.routing = routing;
    }

    pub fn state(&self) -> SessionState {
        self.state
    }
    pub fn input(&self) -> Option<&DeviceId> {
        self.input.as_ref()
    }
    pub fn output(&self) -> Option<&VirtualCable> {
        self.output.as_ref()
    }
    pub fn monitor(&self) -> &MonitorSettings {
        &self.monitor
    }
    pub fn shift(&self) -> ShiftSettings {
        self.shift
    }
    pub fn analysis(&self) -> AnalysisSettings {
        self.analysis
    }

    fn is_cable(&self, id: &DeviceId) -> bool {
        self.output.as_ref().is_some_and(|c| &c.playback == id || &c.recording == id)
    }

    pub fn select_input(&mut self, device: DeviceId) -> Result<(), DomainError> {
        if self.state == SessionState::Running {
            return Err(DomainError::InputChangeWhileRunning);
        }
        if self.is_cable(&device) {
            return Err(DomainError::CableRecordingAsInput);
        }
        self.input = Some(device);
        Ok(())
    }

    /// モニターは動いている間でも替えられる。
    pub fn set_monitor(&mut self, enabled: bool, device: Option<DeviceId>) -> Result<(), DomainError> {
        if let Some(d) = &device {
            if self.is_cable(d) {
                return Err(DomainError::CablePlaybackAsMonitor);
            }
        }
        if enabled && device.is_none() {
            return Err(DomainError::NoMonitorDevice);
        }
        self.monitor.enabled = enabled;
        self.monitor.device = device;
        Ok(())
    }

    pub fn set_monitor_volume(&mut self, volume: MonitorVolume) {
        self.monitor.volume = volume;
    }

    pub fn start(&mut self) -> Result<(), DomainError> {
        if self.state == SessionState::Running {
            return Err(DomainError::AlreadyRunning);
        }
        if self.output.is_none() {
            return Err(DomainError::NoVirtualCable);
        }
        if self.input.is_none() {
            return Err(DomainError::NoInputDevice);
        }
        self.state = SessionState::Running;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), DomainError> {
        if self.state == SessionState::Stopped {
            return Err(DomainError::NotRunning);
        }
        self.state = SessionState::Stopped;
        Ok(())
    }

    pub fn change_shift(&mut self, shift: ShiftSettings) {
        self.shift = shift;
    }

    pub fn change_analysis(&mut self, analysis: AnalysisSettings) {
        self.analysis = analysis;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceInfo;

    fn info(raw: &str, direction: DeviceDirection, is_default: bool) -> DeviceInfo {
        DeviceInfo { raw_id: raw.into(), name: raw.into(), direction, is_default }
    }

    fn list(with_cable: bool, cable_is_default_output: bool) -> DeviceList {
        let devices = vec![
            info("mic", DeviceDirection::Input, true),
            info("cable-rec", DeviceDirection::Input, false),
            info("phones", DeviceDirection::Output, !cable_is_default_output),
            info("cable-play", DeviceDirection::Output, cable_is_default_output),
        ];
        DeviceList::new(devices, with_cable.then_some(("cable-play", "cable-rec")))
    }

    #[test]
    fn starts_with_windows_defaults_and_monitor_off() {
        let devices = list(true, false);
        let s = VoiceSession::new(&devices);
        assert_eq!(s.input().map(DeviceId::raw), Some("mic"));
        assert_eq!(s.monitor().device.as_ref().map(DeviceId::raw), Some("phones"));
        assert!(!s.monitor().enabled);
        assert_eq!(s.output().map(|c| c.playback.raw()), Some("cable-play"));
    }

    /// C6: 既定の再生デバイスが VB-CABLE の再生側なら、モニターの欄は空。
    #[test]
    fn default_monitor_is_empty_when_default_output_is_cable() {
        let devices = list(true, true);
        assert!(VoiceSession::new(&devices).monitor().device.is_none());
    }

    /// C2: 一覧から VB-CABLE の端を外す。
    #[test]
    fn lists_exclude_cable_ends() {
        let devices = list(true, false);
        assert_eq!(devices.inputs().map(|d| d.raw_id.as_str()).collect::<Vec<_>>(), ["mic"]);
        assert_eq!(devices.monitors().map(|d| d.raw_id.as_str()).collect::<Vec<_>>(), ["phones"]);
    }

    /// C4: VB-CABLE が無いと開始できない。
    #[test]
    fn cannot_start_without_cable() {
        let devices = list(false, false);
        let mut s = VoiceSession::new(&devices);
        assert_eq!(s.start(), Err(DomainError::NoVirtualCable));
    }

    /// C2 の規則: 一覧の作りに頼らず、domain でも VB-CABLE の端を選べない。
    #[test]
    fn cable_ends_are_rejected_as_input_and_monitor() {
        let devices = list(true, false);
        let mut s = VoiceSession::new(&devices);
        let rec = devices.id("cable-rec", DeviceDirection::Input).unwrap();
        let play = devices.id("cable-play", DeviceDirection::Output).unwrap();
        assert_eq!(s.select_input(rec), Err(DomainError::CableRecordingAsInput));
        assert_eq!(s.set_monitor(true, Some(play)), Err(DomainError::CablePlaybackAsMonitor));
    }

    #[test]
    fn input_is_fixed_while_running_but_monitor_can_change() {
        let devices = list(true, false);
        let mut s = VoiceSession::new(&devices);
        s.start().unwrap();
        let mic = devices.id("mic", DeviceDirection::Input).unwrap();
        assert_eq!(s.select_input(mic), Err(DomainError::InputChangeWhileRunning));
        let phones = devices.id("phones", DeviceDirection::Output).unwrap();
        assert!(s.set_monitor(true, Some(phones)).is_ok());
        s.stop().unwrap();
    }

    #[test]
    fn monitor_needs_a_device_to_be_enabled() {
        let devices = list(true, true);
        let mut s = VoiceSession::new(&devices);
        assert_eq!(s.set_monitor(true, None), Err(DomainError::NoMonitorDevice));
    }

    #[test]
    fn device_id_cannot_be_made_from_unknown_string() {
        let devices = list(true, false);
        assert!(matches!(
            devices.id("not-in-list", DeviceDirection::Input),
            Err(DomainError::UnknownDevice(_))
        ));
        // 方向が違えば同じ id でも作れない
        assert!(devices.id("mic", DeviceDirection::Output).is_err());
    }
}
