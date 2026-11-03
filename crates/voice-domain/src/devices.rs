//! The device list, and device values made from the list.

use crate::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceDirection {
    Input,
    Output,
}

/// Device information returned by an adapter. `raw_id` is the adapter's own identifier; the domain does not interpret it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub raw_id: String,
    pub name: String,
    pub direction: DeviceDirection,
    pub is_default: bool,
}

/// A value that points to a device. It can only be made from what is in a `DeviceList`,
/// so that an arbitrary string from the screen cannot point to a device.
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

/// The playback and recording sides of the output (VB-CABLE).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualCable {
    pub playback: DeviceId,
    pub recording: DeviceId,
}

/// The device list at a point in time, and the virtual microphone device found in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceList {
    devices: Vec<DeviceInfo>,
    cable: Option<VirtualCable>,
}

impl DeviceList {
    /// Made from the adapter's enumeration. `cable` is given as a `raw_id` in the list.
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

    /// The list with one more input device (for the test source). The added device becomes the default input.
    pub fn with_extra_input(mut self, info: DeviceInfo) -> Self {
        if info.is_default {
            for d in self.devices.iter_mut().filter(|d| d.direction == DeviceDirection::Input) {
                d.is_default = false;
            }
        }
        self.devices.push(info);
        self
    }

    /// The name of the device with `raw_id`.
    pub fn name(&self, raw_id: &str) -> Option<&str> {
        self.devices.iter().find(|d| d.raw_id == raw_id).map(|d| d.name.as_str())
    }

    pub fn cable(&self) -> Option<&VirtualCable> {
        self.cable.as_ref()
    }

    /// Makes a value that points to a device in the list.
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

    /// The input list shown on screen. Leaves out the VB-CABLE ends.
    pub fn inputs(&self) -> impl Iterator<Item = &DeviceInfo> {
        self.devices.iter().filter(|d| d.direction == DeviceDirection::Input && !self.is_cable_end(d))
    }

    /// The output and monitor list shown on screen (all playback devices). Removing the output device from the monitor list is done by the screen.
    pub fn outputs(&self) -> impl Iterator<Item = &DeviceInfo> {
        self.devices.iter().filter(|d| d.direction == DeviceDirection::Output)
    }

    /// The Windows default device. `None` if it is a VB-CABLE end.
    pub fn default_device(&self, direction: DeviceDirection) -> Option<DeviceId> {
        self.devices
            .iter()
            .find(|d| d.direction == direction && d.is_default)
            .filter(|d| !self.is_cable_end(d))
            .map(|d| DeviceId { raw: d.raw_id.clone(), direction })
    }
}
