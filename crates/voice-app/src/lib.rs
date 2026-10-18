//! application: use case（`VoiceStudio`）。port を generics で受け取り、
//! `VoiceSession` のメソッドを通してだけ状態を変える。

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
    #[error("機器の一覧を取れませんでした: {0}")]
    Devices(Box<dyn std::error::Error + Send + Sync>),
    #[error("音声の処理でエラーが起きました: {0}")]
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
    /// 起動したときの状態を作る。機器の一覧はここで 1 回取る。
    pub fn new(devices: D, pipeline: P) -> Result<Self, AppError> {
        let list = devices.list().map_err(|e| AppError::Devices(Box::new(e)))?;
        let session = VoiceSession::new(&list);
        Ok(Self { devices, pipeline, list, session, analysis_display: true })
    }

    pub fn session(&self) -> &VoiceSession {
        &self.session
    }

    /// 最後に取った機器の一覧。
    pub fn devices(&self) -> &DeviceList {
        &self.list
    }

    /// 組み立てた adapter の中を見る（試験と画面の状態の表示に使う）。
    pub fn pipeline(&self) -> &P {
        &self.pipeline
    }

    /// 機器の一覧を取り直す。止まっている間だけ、選んでいる機器を新しい一覧に合わせ直す。
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

    /// 前に選んだ機器を戻す。一覧に無い機器や規則に合わない機器は飛ばし、既定のままにする。
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

    /// 出力とモニターに、変える前と後のどちらの音を流すかを切り替える。
    pub fn set_routing(&mut self, routing: Routing) -> Result<(), AppError> {
        self.session.set_routing(routing);
        if self.session.state() == SessionState::Running {
            self.pipeline.set_routing(routing).map_err(|e| AppError::Pipeline(Box::new(e)))?;
        }
        Ok(())
    }

    /// 表示のための分析を動かすか（簡易表示では止める）。止まっている間も覚えておき、開始のときに渡す。
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

    /// C4: VB-CABLE が無く出力を選んでいなければ開始せず、pipeline も呼ばない。
    #[test]
    fn start_without_output_fails_before_pipeline() {
        let (mut s, calls, _) = studio(false);
        assert!(matches!(s.start(), Err(AppError::Domain(DomainError::NoOutputDevice))));
        assert!(calls.borrow().is_empty());
    }

    /// 覚えた機器を戻す。一覧に無い機器は飛ばして既定のままにする。
    #[test]
    fn restore_devices_applies_known_ids_and_ignores_missing() {
        let (mut s, _, _) = studio(true);
        s.restore_devices(Some("usb-mic"), Some("phones"), Some("gone"));
        assert_eq!(s.session().input().map(|d| d.raw()), Some("usb-mic"));
        assert_eq!(s.session().output().map(|d| d.raw()), Some("phones"));
        // モニターの既定は phones だったが、出力と同じなので外れ、覚えた gone は一覧に無いので空のまま
        assert!(s.session().monitor().device.is_none());
        s.restore_devices(Some("missing"), None, None);
        assert_eq!(s.session().input().map(|d| d.raw()), Some("usb-mic"));
    }

    /// 画面から来た一覧に無い文字列は、use case まで届いても機器にならない。
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

    /// 起動し直さずに一覧を取り直したとき、残っている機器の選択は保ち、消えた機器は外す。
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
        // 出力の VB-CABLE が消えたので、出力は空になる
        assert!(s.session().output().is_none());
    }
}
