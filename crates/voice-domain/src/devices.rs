//! 機器の一覧と、一覧から作る機器の値。

use crate::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceDirection {
    Input,
    Output,
}

/// adapter が返す機器の情報。`raw_id` は adapter の中の識別子で、domain は中身を解さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub raw_id: String,
    pub name: String,
    pub direction: DeviceDirection,
    pub is_default: bool,
}

/// 機器を指す値。`DeviceList` の中にあるものからしか作れない。
/// 画面から来た任意の文字列で機器を指せないようにするため。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId {
    raw: String,
    direction: DeviceDirection,
}

impl DeviceId {
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn direction(&self) -> DeviceDirection {
        self.direction
    }
}

/// 出力の先（VB-CABLE）の再生側と録音側。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualCable {
    pub playback: DeviceId,
    pub recording: DeviceId,
}

/// その時点の機器の一覧と、見つかった仮想マイクの機器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceList {
    devices: Vec<DeviceInfo>,
    cable: Option<VirtualCable>,
}

impl DeviceList {
    /// adapter の列挙の結果から作る。`cable` は一覧の中の `raw_id` で指す。
    pub fn new(devices: Vec<DeviceInfo>, cable: Option<(&str, &str)>) -> Self {
        let mut list = Self { devices, cable: None };
        list.cable = cable.and_then(|(playback, recording)| {
            Some(VirtualCable {
                playback: list.id(playback, DeviceDirection::Output).ok()?,
                recording: list.id(recording, DeviceDirection::Input).ok()?,
            })
        });
        list
    }

    pub fn cable(&self) -> Option<&VirtualCable> {
        self.cable.as_ref()
    }

    /// 一覧の中の機器を指す値を作る。
    pub fn id(&self, raw_id: &str, direction: DeviceDirection) -> Result<DeviceId, DomainError> {
        self.devices
            .iter()
            .find(|d| d.raw_id == raw_id && d.direction == direction)
            .map(|d| DeviceId { raw: d.raw_id.clone(), direction })
            .ok_or_else(|| DomainError::UnknownDevice(raw_id.to_string()))
    }

    fn is_cable_end(&self, info: &DeviceInfo) -> bool {
        self.cable
            .as_ref()
            .is_some_and(|c| c.playback.raw == info.raw_id || c.recording.raw == info.raw_id)
    }

    /// 画面に出す入力の一覧。VB-CABLE の端を外す。
    pub fn inputs(&self) -> impl Iterator<Item = &DeviceInfo> {
        self.devices.iter().filter(|d| d.direction == DeviceDirection::Input && !self.is_cable_end(d))
    }

    /// 画面に出すモニターの一覧。VB-CABLE の端を外す。
    pub fn monitors(&self) -> impl Iterator<Item = &DeviceInfo> {
        self.devices.iter().filter(|d| d.direction == DeviceDirection::Output && !self.is_cable_end(d))
    }

    /// Windows の既定の機器。VB-CABLE の端なら `None`。
    pub fn default_device(&self, direction: DeviceDirection) -> Option<DeviceId> {
        self.devices
            .iter()
            .find(|d| d.direction == direction && d.is_default)
            .filter(|d| !self.is_cable_end(d))
            .map(|d| DeviceId { raw: d.raw_id.clone(), direction })
    }
}
