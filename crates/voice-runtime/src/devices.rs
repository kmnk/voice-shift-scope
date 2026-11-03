use cpal::traits::{DeviceTrait, HostTrait};

/// Whether a device is a recording or a playback device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDirection {
    Input,
    Output,
}

/// Device information returned by cpal. Besides the name, it keeps driver, manufacturer, and extended,
/// to check the clues for finding VB-CABLE.
#[derive(Debug, Clone)]
pub struct ProbedDevice {
    pub direction: DeviceDirection,
    pub id: String,
    pub name: String,
    pub manufacturer: Option<String>,
    pub driver: Option<String>,
    pub extended: Vec<String>,
    pub is_default: bool,
}

/// The device list of the default host.
#[derive(Debug, Clone)]
pub struct DeviceListing {
    pub host: String,
    pub devices: Vec<ProbedDevice>,
}

/// Enumerates the recording and playback devices of the default host. Does not open any device.
pub fn probe_devices() -> Result<DeviceListing, cpal::Error> {
    let host = cpal::default_host();
    let default_input = host.default_input_device().and_then(|d| d.id().ok());
    let default_output = host.default_output_device().and_then(|d| d.id().ok());

    let mut devices = Vec::new();
    for device in host.input_devices()? {
        devices.push(probe(&device, DeviceDirection::Input, default_input.as_ref())?);
    }
    for device in host.output_devices()? {
        devices.push(probe(&device, DeviceDirection::Output, default_output.as_ref())?);
    }
    Ok(DeviceListing {
        host: format!("{:?}", host.id()),
        devices,
    })
}

fn probe(
    device: &cpal::Device,
    direction: DeviceDirection,
    default_id: Option<&cpal::DeviceId>,
) -> Result<ProbedDevice, cpal::Error> {
    let id = device.id()?;
    let description = device.description()?;
    Ok(ProbedDevice {
        direction,
        is_default: default_id == Some(&id),
        id: id.to_string(),
        name: description.name().to_string(),
        manufacturer: description.manufacturer().map(str::to_string),
        driver: description.driver().map(str::to_string),
        extended: description.extended().map(str::to_string).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Even on CI without devices, enumeration itself is not an error.
    #[test]
    fn probe_devices_does_not_fail() {
        let listing = probe_devices().expect("can enumerate");
        for d in &listing.devices {
            println!("{:?} {:?} {:?} driver={:?}", d.direction, d.name, d.id, d.driver);
        }
    }
}
