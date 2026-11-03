//! Finds the output (VB-CABLE) in the device list.
//!
//! The first part of the name ("CABLE Input", "Speakers", and so on) varies with the version and installation, and users can change it,
//! so it searches by the driver name cpal returns. Only when the driver is not available does it look inside the parentheses of the name.

use crate::{DeviceDirection, ProbedDevice};

/// The VB-CABLE driver name.
pub const VB_CABLE_DRIVER: &str = "VB-Audio Virtual Cable";

/// The VB-CABLE playback and recording sides found.
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

/// Finds one VB-CABLE playback side and one recording side in the list. `None` if either is missing.
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

    /// Found even in the form cpal returned on a CI runner (playback side named Speakers).
    #[test]
    fn finds_cable_by_driver_as_seen_on_ci_runner() {
        let devices = vec![
            device(DeviceDirection::Input, "Microphone (USB)", Some("USB Audio")),
            device(DeviceDirection::Input, "CABLE Output (VB-Audio Virtual Cable)", Some(VB_CABLE_DRIVER)),
            device(DeviceDirection::Output, "Speakers (VB-Audio Virtual Cable)", Some(VB_CABLE_DRIVER)),
            device(DeviceDirection::Output, "Headphones (USB)", Some("USB Audio")),
        ];
        let cable = find_virtual_cable(&devices).expect("found");
        assert_eq!(cable.playback.name, "Speakers (VB-Audio Virtual Cable)");
        assert_eq!(cable.recording.name, "CABLE Output (VB-Audio Virtual Cable)");
    }

    /// Found when the user changes the first part of the name, as long as the driver is the same.
    #[test]
    fn finds_renamed_cable_by_driver() {
        let devices = vec![
            device(DeviceDirection::Input, "Virtual mic", Some(VB_CABLE_DRIVER)),
            device(DeviceDirection::Output, "Voice out", Some(VB_CABLE_DRIVER)),
        ];
        assert!(find_virtual_cable(&devices).is_some());
    }

    /// Not found with only one side or with a different driver.
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

    /// When the driver is not available, searches inside the parentheses of the name.
    #[test]
    fn falls_back_to_name_suffix_without_driver() {
        let devices = vec![
            device(DeviceDirection::Input, "CABLE Output (VB-Audio Virtual Cable)", None),
            device(DeviceDirection::Output, "CABLE Input (VB-Audio Virtual Cable)", None),
        ];
        assert!(find_virtual_cable(&devices).is_some());
    }
}
