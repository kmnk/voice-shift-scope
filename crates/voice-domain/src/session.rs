//! One run of "take the microphone in and send it to the virtual microphone".

use crate::{AnalysisSettings, DeviceDirection, DeviceId, DeviceList, DomainError, ShiftSettings, VirtualCable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Stopped,
    Running,
}

/// Monitor volume (0 to 1).
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

/// Monitor settings. Off at start.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorSettings {
    pub enabled: bool,
    pub device: Option<DeviceId>,
    pub volume: MonitorVolume,
}

/// The audio sent to the output or monitor. The original is delayed as much as the shifted audio (so switching does not jump in time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioSource {
    #[default]
    Shifted,
    Original,
}

/// Which audio goes to the output (virtual microphone) and the monitor. Both are shifted at start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Routing {
    pub output: AudioSource,
    pub monitor: AudioSource,
}

/// The aggregate. Use cases change the state only through these methods.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSession {
    state: SessionState,
    input: Option<DeviceId>,
    /// Where the voice goes (a playback device). Defaults to the playback side of the VB-CABLE found (the user made it selectable on 2026-10-18)
    output: Option<DeviceId>,
    /// The VB-CABLE found. Used for the input rule (its recording side cannot be chosen) and the default output
    cable: Option<VirtualCable>,
    monitor: MonitorSettings,
    routing: Routing,
    shift: ShiftSettings,
    analysis: AnalysisSettings,
}

impl VoiceSession {
    /// The state at start. The input and monitor are the Windows default devices (empty if a VB-CABLE end),
    /// and the output is the playback side of the VB-CABLE found (empty if none). The monitor is never the same device as the output.
    pub fn new(devices: &DeviceList) -> Self {
        let cable = devices.cable().cloned();
        let output = cable.as_ref().map(|c| c.playback.clone());
        let monitor = devices.default_device(DeviceDirection::Output).filter(|d| Some(d) != output.as_ref());
        Self {
            state: SessionState::Stopped,
            input: devices.default_device(DeviceDirection::Input),
            output,
            cable,
            monitor: MonitorSettings { enabled: false, device: monitor, volume: MonitorVolume::DEFAULT },
            routing: Routing::default(),
            shift: ShiftSettings::default(),
            analysis: AnalysisSettings::default(),
        }
    }

    pub fn routing(&self) -> Routing {
        self.routing
    }

    /// The audio source can be switched while running.
    pub fn set_routing(&mut self, routing: Routing) {
        self.routing = routing;
    }

    pub fn state(&self) -> SessionState {
        self.state
    }
    pub fn input(&self) -> Option<&DeviceId> {
        self.input.as_ref()
    }
    pub fn output(&self) -> Option<&DeviceId> {
        self.output.as_ref()
    }
    pub fn cable(&self) -> Option<&VirtualCable> {
        self.cable.as_ref()
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

    pub fn select_input(&mut self, device: DeviceId) -> Result<(), DomainError> {
        if self.state == SessionState::Running {
            return Err(DomainError::InputChangeWhileRunning);
        }
        // The VB-CABLE recording side cannot be the input, so the shifted voice does not loop back into the input
        if self.cable.as_ref().is_some_and(|c| c.recording == device) {
            return Err(DomainError::CableRecordingAsInput);
        }
        self.input = Some(device);
        Ok(())
    }

    /// Chooses where the voice goes. Only while stopped. If the monitor is the same device, the monitor is cleared.
    pub fn select_output(&mut self, device: DeviceId) -> Result<(), DomainError> {
        if self.state == SessionState::Running {
            return Err(DomainError::OutputChangeWhileRunning);
        }
        if self.monitor.device.as_ref() == Some(&device) {
            self.monitor.device = None;
            self.monitor.enabled = false;
        }
        self.output = Some(device);
        Ok(())
    }

    /// The monitor can be changed while running. It cannot be the same device as the output.
    pub fn set_monitor(&mut self, enabled: bool, device: Option<DeviceId>) -> Result<(), DomainError> {
        if device.is_some() && device == self.output {
            return Err(DomainError::MonitorSameAsOutput);
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
            return Err(DomainError::NoOutputDevice);
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
            info("speakers", DeviceDirection::Output, false),
            info("cable-play", DeviceDirection::Output, cable_is_default_output),
        ];
        DeviceList::new(devices, with_cable.then_some(("cable-play", "cable-rec")))
    }

    #[test]
    fn starts_with_cable_as_output_and_windows_defaults_elsewhere() {
        let devices = list(true, false);
        let s = VoiceSession::new(&devices);
        assert_eq!(s.input().map(DeviceId::raw), Some("mic"));
        assert_eq!(s.output().map(DeviceId::raw), Some("cable-play"));
        assert_eq!(s.monitor().device.as_ref().map(DeviceId::raw), Some("phones"));
        assert!(!s.monitor().enabled);
    }

    /// C6: if the default playback device is the VB-CABLE playback side (= the output), the monitor is empty.
    #[test]
    fn default_monitor_is_empty_when_default_output_is_the_output() {
        let devices = list(true, true);
        assert!(VoiceSession::new(&devices).monitor().device.is_none());
    }

    #[test]
    fn input_list_excludes_cable_recording() {
        let devices = list(true, false);
        assert_eq!(devices.inputs().map(|d| d.raw_id.as_str()).collect::<Vec<_>>(), ["mic"]);
        assert_eq!(devices.outputs().count(), 3);
    }

    /// C4: without VB-CABLE, the output is empty and the session cannot start. Choosing another playback device lets it start.
    #[test]
    fn without_cable_output_must_be_chosen() {
        let devices = list(false, false);
        let mut s = VoiceSession::new(&devices);
        assert_eq!(s.start(), Err(DomainError::NoOutputDevice));
        s.select_output(devices.id("speakers", DeviceDirection::Output).unwrap()).unwrap();
        assert!(s.start().is_ok());
    }

    #[test]
    fn cable_recording_cannot_be_input_and_monitor_cannot_be_output() {
        let devices = list(true, false);
        let mut s = VoiceSession::new(&devices);
        let rec = devices.id("cable-rec", DeviceDirection::Input).unwrap();
        let play = devices.id("cable-play", DeviceDirection::Output).unwrap();
        assert_eq!(s.select_input(rec), Err(DomainError::CableRecordingAsInput));
        assert_eq!(s.set_monitor(true, Some(play)), Err(DomainError::MonitorSameAsOutput));
    }

    /// Changing the output to the monitor's device clears the monitor.
    #[test]
    fn choosing_monitor_device_as_output_clears_monitor() {
        let devices = list(true, false);
        let mut s = VoiceSession::new(&devices);
        let phones = devices.id("phones", DeviceDirection::Output).unwrap();
        s.set_monitor(true, Some(phones.clone())).unwrap();
        s.select_output(phones).unwrap();
        assert!(s.monitor().device.is_none() && !s.monitor().enabled);
    }

    #[test]
    fn devices_are_fixed_while_running_but_monitor_can_change() {
        let devices = list(true, false);
        let mut s = VoiceSession::new(&devices);
        s.start().unwrap();
        let mic = devices.id("mic", DeviceDirection::Input).unwrap();
        assert_eq!(s.select_input(mic), Err(DomainError::InputChangeWhileRunning));
        let speakers = devices.id("speakers", DeviceDirection::Output).unwrap();
        assert_eq!(s.select_output(speakers.clone()), Err(DomainError::OutputChangeWhileRunning));
        assert!(s.set_monitor(true, Some(speakers)).is_ok());
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
        assert!(matches!(devices.id("not-in-list", DeviceDirection::Input), Err(DomainError::UnknownDevice(_))));
        assert!(devices.id("mic", DeviceDirection::Output).is_err());
    }
}
