//! application: use cases (`VoiceStudio`). Takes the ports as generics and
//! changes the state only through the methods of `VoiceSession`.

use thiserror::Error;
use voice_domain::ports::{AudioDevices, AudioPipeline, PipelineConfig};
use voice_domain::{
    AnalysisSettings, DeviceDirection, DeviceList, DomainError, MonitorVolume, Routing, SessionState, ShiftSettings,
    VoiceSession,
};

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("could not list the devices: {0}")]
    Devices(Box<dyn std::error::Error + Send + Sync>),
    #[error("audio error: {0}")]
    Pipeline(Box<dyn std::error::Error + Send + Sync>),
}

pub struct VoiceStudio<D: AudioDevices, P: AudioPipeline> {
    devices: D,
    pipeline: P,
    list: DeviceList,
    session: VoiceSession,
    analysis_display: bool,
}

impl<D: AudioDevices, P: AudioPipeline> VoiceStudio<D, P> {
    /// Makes the state at start. The device list is fetched once here.
    pub fn new(devices: D, pipeline: P) -> Result<Self, AppError> {
        let list = devices.list().map_err(|e| AppError::Devices(Box::new(e)))?;
        let session = VoiceSession::new(&list);
        Ok(Self { devices, pipeline, list, session, analysis_display: true })
    }

    pub fn session(&self) -> &VoiceSession {
        &self.session
    }

    /// The device list fetched last.
    pub fn devices(&self) -> &DeviceList {
        &self.list
    }

    /// Looks inside the assembled adapters (for tests and for showing the state on screen).
    pub fn pipeline(&self) -> &P {
        &self.pipeline
    }

    /// Fetches the device list again. Only while stopped, the chosen devices are matched to the new list.
    pub fn list_devices(&mut self) -> Result<&DeviceList, AppError> {
        let list = self.devices.list().map_err(|e| AppError::Devices(Box::new(e)))?;
        if self.session.state() == SessionState::Stopped {
            let previous = self.session.clone();
            let mut session = VoiceSession::new(&list);
            if let Some(output) = previous.output().and_then(|d| list.id(d.raw(), DeviceDirection::Output).ok()) {
                let _ = session.select_output(output);
            }
            if let Some(input) = previous.input().and_then(|d| list.id(d.raw(), DeviceDirection::Input).ok()) {
                let _ = session.select_input(input);
            }
            let monitor = previous.monitor();
            let device = monitor.device.as_ref().and_then(|d| list.id(d.raw(), DeviceDirection::Output).ok());
            let _ = session.set_monitor(monitor.enabled && device.is_some(), device);
            session.set_monitor_volume(monitor.volume);
            session.set_routing(previous.routing());
            session.change_shift(previous.shift());
            session.change_analysis(previous.analysis());
            self.session = session;
        }
        self.list = list;
        Ok(&self.list)
    }

    pub fn select_output(&mut self, raw_id: &str) -> Result<(), AppError> {
        let id = self.list.id(raw_id, DeviceDirection::Output)?;
        Ok(self.session.select_output(id)?)
    }

    /// Restores the devices chosen before. Devices not in the list or not allowed by the rules are skipped, keeping the default.
    pub fn restore_devices(&mut self, input: Option<&str>, output: Option<&str>, monitor: Option<&str>) {
        if let Some(id) = output.and_then(|r| self.list.id(r, DeviceDirection::Output).ok()) {
            let _ = self.session.select_output(id);
        }
        if let Some(id) = input.and_then(|r| self.list.id(r, DeviceDirection::Input).ok()) {
            let _ = self.session.select_input(id);
        }
        if let Some(id) = monitor.and_then(|r| self.list.id(r, DeviceDirection::Output).ok()) {
            let enabled = self.session.monitor().enabled;
            let _ = self.session.set_monitor(enabled, Some(id));
        }
    }

    pub fn select_input(&mut self, raw_id: &str) -> Result<(), AppError> {
        let id = self.list.id(raw_id, DeviceDirection::Input)?;
        Ok(self.session.select_input(id)?)
    }

    pub fn set_monitor(&mut self, enabled: bool, raw_id: Option<&str>) -> Result<(), AppError> {
        let id = raw_id.map(|r| self.list.id(r, DeviceDirection::Output)).transpose()?;
        self.session.set_monitor(enabled, id)?;
        if self.session.state() == SessionState::Running {
            self.pipeline
                .set_monitor(self.session.monitor().clone())
                .map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        Ok(())
    }

    pub fn set_monitor_volume(&mut self, volume: MonitorVolume) -> Result<(), AppError> {
        self.session.set_monitor_volume(volume);
        if self.session.state() == SessionState::Running {
            self.pipeline
                .set_monitor(self.session.monitor().clone())
                .map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        Ok(())
    }

    /// Switches whether the output and monitor get the original or the shifted audio.
    pub fn set_routing(&mut self, routing: Routing) -> Result<(), AppError> {
        self.session.set_routing(routing);
        if self.session.state() == SessionState::Running {
            self.pipeline.set_routing(routing).map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        Ok(())
    }

    /// Whether to run the analysis for display (off in the compact view). Remembered while stopped and passed on start.
    pub fn set_analysis_display(&mut self, active: bool) -> Result<(), AppError> {
        self.analysis_display = active;
        if self.session.state() == SessionState::Running {
            self.pipeline.set_analysis_display(active).map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), AppError> {
        let mut next = self.session.clone();
        next.start()?;
        let config = PipelineConfig {
            input: next.input().cloned().ok_or(DomainError::NoInputDevice)?,
            output: next.output().cloned().ok_or(DomainError::NoOutputDevice)?,
            monitor: next.monitor().clone(),
            routing: next.routing(),
            shift: next.shift(),
            analysis: next.analysis(),
        };
        self.pipeline.start(config).map_err(|e| AppError::Pipeline(Box::new(e)))?;
        if !self.analysis_display {
            self.pipeline.set_analysis_display(false).map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        self.session = next;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), AppError> {
        self.session.stop()?;
        self.pipeline.stop().map_err(|e| AppError::Pipeline(Box::new(e)))
    }

    pub fn change_shift(&mut self, shift: ShiftSettings) -> Result<(), AppError> {
        self.session.change_shift(shift);
        if self.session.state() == SessionState::Running {
            self.pipeline.set_shift(shift).map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        Ok(())
    }

    pub fn change_analysis(&mut self, analysis: AnalysisSettings) -> Result<(), AppError> {
        self.session.change_analysis(analysis);
        if self.session.state() == SessionState::Running {
            self.pipeline.set_analysis(analysis).map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use voice_domain::{DeviceInfo, FormantShift, MonitorSettings, PitchShift};

    #[derive(Debug, Error)]
    #[error("fake")]
    struct FakeError;

    struct FakeDevices(Rc<RefCell<DeviceList>>);

    impl AudioDevices for FakeDevices {
        type Error = FakeError;
        fn list(&self) -> Result<DeviceList, FakeError> {
            Ok(self.0.borrow().clone())
        }
    }

    #[derive(Debug, PartialEq)]
    enum Call {
        Start(PipelineConfig),
        Stop,
        Shift(ShiftSettings),
        Analysis(AnalysisSettings),
        Monitor(MonitorSettings),
        Routing(Routing),
    }

    #[derive(Default)]
    struct FakePipeline {
        calls: Rc<RefCell<Vec<Call>>>,
        fail_start: bool,
    }

    impl AudioPipeline for FakePipeline {
        type Error = FakeError;
        fn start(&mut self, c: PipelineConfig) -> Result<(), FakeError> {
            if self.fail_start {
                return Err(FakeError);
            }
            self.calls.borrow_mut().push(Call::Start(c));
            Ok(())
        }
        fn stop(&mut self) -> Result<(), FakeError> {
            self.calls.borrow_mut().push(Call::Stop);
            Ok(())
        }
        fn set_shift(&mut self, s: ShiftSettings) -> Result<(), FakeError> {
            self.calls.borrow_mut().push(Call::Shift(s));
            Ok(())
        }
        fn set_analysis(&mut self, a: AnalysisSettings) -> Result<(), FakeError> {
            self.calls.borrow_mut().push(Call::Analysis(a));
            Ok(())
        }
        fn set_monitor(&mut self, m: MonitorSettings) -> Result<(), FakeError> {
            self.calls.borrow_mut().push(Call::Monitor(m));
            Ok(())
        }
        fn set_routing(&mut self, r: Routing) -> Result<(), FakeError> {
            self.calls.borrow_mut().push(Call::Routing(r));
            Ok(())
        }
        fn set_analysis_display(&mut self, _active: bool) -> Result<(), FakeError> {
            Ok(())
        }
    }

    fn info(raw: &str, direction: DeviceDirection, is_default: bool) -> DeviceInfo {
        DeviceInfo { raw_id: raw.into(), name: raw.into(), direction, is_default }
    }

    fn list(with_cable: bool) -> DeviceList {
        DeviceList::new(
            vec![
                info("mic", DeviceDirection::Input, true),
                info("usb-mic", DeviceDirection::Input, false),
                info("cable-rec", DeviceDirection::Input, false),
                info("phones", DeviceDirection::Output, true),
                info("cable-play", DeviceDirection::Output, false),
            ],
            with_cable.then_some(("cable-play", "cable-rec")),
        )
    }

    fn studio(with_cable: bool) -> (VoiceStudio<FakeDevices, FakePipeline>, Rc<RefCell<Vec<Call>>>, Rc<RefCell<DeviceList>>) {
        let shared = Rc::new(RefCell::new(list(with_cable)));
        let pipeline = FakePipeline::default();
        let calls = pipeline.calls.clone();
        (VoiceStudio::new(FakeDevices(shared.clone()), pipeline).unwrap(), calls, shared)
    }

    #[test]
    fn start_passes_cable_and_selected_input_to_pipeline() {
        let (mut s, calls, _) = studio(true);
        s.select_input("usb-mic").unwrap();
        s.start().unwrap();
        match &calls.borrow()[0] {
            Call::Start(c) => {
                assert_eq!(c.input.raw(), "usb-mic");
                assert_eq!(c.output.raw(), "cable-play");
            }
            other => panic!("{other:?}"),
        }
    }

    /// C4: without VB-CABLE and with no output chosen, it does not start and does not call the pipeline.
    #[test]
    fn start_without_output_fails_before_pipeline() {
        let (mut s, calls, _) = studio(false);
        assert!(matches!(s.start(), Err(AppError::Domain(DomainError::NoOutputDevice))));
        assert!(calls.borrow().is_empty());
    }

    /// Restores remembered devices. Devices not in the list are skipped, keeping the default.
    #[test]
    fn restore_devices_applies_known_ids_and_ignores_missing() {
        let (mut s, _, _) = studio(true);
        s.restore_devices(Some("usb-mic"), Some("phones"), Some("gone"));
        assert_eq!(s.session().input().map(|d| d.raw()), Some("usb-mic"));
        assert_eq!(s.session().output().map(|d| d.raw()), Some("phones"));
        // The monitor defaulted to phones but was cleared because it is the output; the remembered gone is not in the list, so it stays empty
        assert!(s.session().monitor().device.is_none());
        s.restore_devices(Some("missing"), None, None);
        assert_eq!(s.session().input().map(|d| d.raw()), Some("usb-mic"));
    }

    /// A string from the screen that is not in the list does not become a device, even if it reaches the use case.
    #[test]
    fn unknown_device_string_is_rejected() {
        let (mut s, _, _) = studio(true);
        assert!(matches!(s.select_input("C:\\evil"), Err(AppError::Domain(DomainError::UnknownDevice(_)))));
        assert!(matches!(s.select_input("cable-rec"), Err(AppError::Domain(DomainError::CableRecordingAsInput))));
    }

    #[test]
    fn changes_while_running_reach_pipeline_and_not_while_stopped() {
        let (mut s, calls, _) = studio(true);
        let shift = ShiftSettings { pitch: PitchShift::new(5.0).unwrap(), formant: FormantShift::new(1.1).unwrap() };
        s.change_shift(shift).unwrap();
        assert!(calls.borrow().is_empty());
        s.start().unwrap();
        s.change_shift(shift).unwrap();
        s.set_monitor(true, Some("phones")).unwrap();
        s.set_monitor_volume(MonitorVolume::new(0.5).unwrap()).unwrap();
        s.stop().unwrap();
        let calls = calls.borrow();
        assert!(matches!(calls[0], Call::Start(ref c) if c.shift == shift));
        assert_eq!(calls[1], Call::Shift(shift));
        assert!(matches!(calls[2], Call::Monitor(ref m) if m.enabled));
        assert!(matches!(calls[3], Call::Monitor(ref m) if m.volume.get() == 0.5));
        assert_eq!(calls[4], Call::Stop);
    }

    #[test]
    fn routing_is_kept_while_stopped_and_sent_while_running() {
        use voice_domain::AudioSource;
        let (mut s, calls, _) = studio(true);
        let original_out = Routing { output: AudioSource::Original, monitor: AudioSource::Shifted };
        s.set_routing(original_out).unwrap();
        assert!(calls.borrow().is_empty());
        s.start().unwrap();
        assert!(matches!(calls.borrow()[0], Call::Start(ref c) if c.routing == original_out));
        let both_original = Routing { output: AudioSource::Original, monitor: AudioSource::Original };
        s.set_routing(both_original).unwrap();
        assert_eq!(calls.borrow()[1], Call::Routing(both_original));
    }

    #[test]
    fn failed_pipeline_start_leaves_session_stopped() {
        let shared = Rc::new(RefCell::new(list(true)));
        let pipeline = FakePipeline { fail_start: true, ..Default::default() };
        let mut s = VoiceStudio::new(FakeDevices(shared), pipeline).unwrap();
        assert!(matches!(s.start(), Err(AppError::Pipeline(_))));
        assert_eq!(s.session().state(), SessionState::Stopped);
    }

    /// When the list is fetched again without restarting, the remaining chosen devices are kept and removed devices are cleared.
    #[test]
    fn relisting_keeps_selection_and_drops_missing_devices() {
        let (mut s, _, shared) = studio(true);
        s.select_input("usb-mic").unwrap();
        s.list_devices().unwrap();
        assert_eq!(s.session().input().map(|d| d.raw()), Some("usb-mic"));
        *shared.borrow_mut() = DeviceList::new(
            vec![info("mic", DeviceDirection::Input, true), info("phones", DeviceDirection::Output, true)],
            None,
        );
        s.list_devices().unwrap();
        assert_eq!(s.session().input().map(|d| d.raw()), Some("mic"));
        // The output VB-CABLE was removed, so the output becomes empty
        assert!(s.session().output().is_none());
    }
}
