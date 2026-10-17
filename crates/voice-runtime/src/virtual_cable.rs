//! 出力の先（VB-CABLE）を機器の一覧から見つける。
//!
//! 名前の前半（「CABLE Input」「Speakers」など）は版や入れ方で変わり、利用者も変えられるので、
//! cpal が返す driver の名前で探す。driver が取れないときに限り、名前の括弧の中を見る。

use crate::{DeviceDirection, ProbedDevice};

/// VB-CABLE の driver の名前。
pub const VB_CABLE_DRIVER: &str = "VB-Audio Virtual Cable";

/// 見つかった VB-CABLE の再生側と録音側。
#[derive(Debug, Clone)]
pub struct VirtualCable {
    pub playback: ProbedDevice,
    pub recording: ProbedDevice,
}

fn is_vb_cable(device: &ProbedDevice) -> bool {
    match device.driver.as_deref() {
        Some(driver) => driver == VB_CABLE_DRIVER,
        None => device.name.ends_with(&format!("({VB_CABLE_DRIVER})")),
    }
}

/// 一覧から VB-CABLE の再生側と録音側を 1 つずつ探す。どちらかが無ければ `None`。
pub fn find_virtual_cable(devices: &[ProbedDevice]) -> Option<VirtualCable> {
    let pick = |direction| {
        devices
            .iter()
            .find(|d| d.direction == direction && is_vb_cable(d))
            .cloned()
    };
    Some(VirtualCable {
        playback: pick(DeviceDirection::Output)?,
        recording: pick(DeviceDirection::Input)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(direction: DeviceDirection, name: &str, driver: Option<&str>) -> ProbedDevice {
        ProbedDevice {
            direction,
            id: format!("id:{name}"),
            name: name.to_string(),
            manufacturer: None,
            driver: driver.map(str::to_string),
            extended: Vec::new(),
            is_default: false,
        }
    }

    /// CI のランナーで cpal が返した形（再生側の名前が Speakers）でも見つかる。
    #[test]
    fn finds_cable_by_driver_as_seen_on_ci_runner() {
        let devices = vec![
            device(DeviceDirection::Input, "Microphone (USB)", Some("USB Audio")),
            device(DeviceDirection::Input, "CABLE Output (VB-Audio Virtual Cable)", Some(VB_CABLE_DRIVER)),
            device(DeviceDirection::Output, "Speakers (VB-Audio Virtual Cable)", Some(VB_CABLE_DRIVER)),
            device(DeviceDirection::Output, "Headphones (USB)", Some("USB Audio")),
        ];
        let cable = find_virtual_cable(&devices).expect("見つかる");
        assert_eq!(cable.playback.name, "Speakers (VB-Audio Virtual Cable)");
        assert_eq!(cable.recording.name, "CABLE Output (VB-Audio Virtual Cable)");
    }

    /// 利用者が名前の前半を変えても、driver が同じなら見つかる。
    #[test]
    fn finds_renamed_cable_by_driver() {
        let devices = vec![
            device(DeviceDirection::Input, "仮想マイク", Some(VB_CABLE_DRIVER)),
            device(DeviceDirection::Output, "声の出口", Some(VB_CABLE_DRIVER)),
        ];
        assert!(find_virtual_cable(&devices).is_some());
    }

    /// 片側しか無い・別の driver のときは見つからない。
    #[test]
    fn missing_side_or_other_driver_is_none() {
        let only_output = vec![device(DeviceDirection::Output, "CABLE Input (VB-Audio Virtual Cable)", Some(VB_CABLE_DRIVER))];
        assert!(find_virtual_cable(&only_output).is_none());
        let other = vec![
            device(DeviceDirection::Input, "CABLE Output (VB-Audio Virtual Cable)", Some("Other")),
            device(DeviceDirection::Output, "CABLE Input (VB-Audio Virtual Cable)", Some("Other")),
        ];
        assert!(find_virtual_cable(&other).is_none());
    }

    /// driver が取れないときは、名前の括弧の中で探す。
    #[test]
    fn falls_back_to_name_suffix_without_driver() {
        let devices = vec![
            device(DeviceDirection::Input, "CABLE Output (VB-Audio Virtual Cable)", None),
            device(DeviceDirection::Output, "CABLE Input (VB-Audio Virtual Cable)", None),
        ];
        assert!(find_virtual_cable(&devices).is_some());
    }
}
