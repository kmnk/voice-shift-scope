use cpal::traits::{DeviceTrait, HostTrait};

/// 機器が録音側か再生側か。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDirection {
    Input,
    Output,
}

/// cpal が返した機器の情報。VB-CABLE を見つける手掛かりを確かめるため、
/// 名前のほかに driver・manufacturer・extended も持つ。
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

/// 既定の host の機器の一覧。
#[derive(Debug, Clone)]
pub struct DeviceListing {
    pub host: String,
    pub devices: Vec<ProbedDevice>,
}

/// 既定の host の録音と再生の機器を列挙する。機器を開かない。
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

    /// 機器が無い CI でも、列挙そのものは誤りにならない。
    #[test]
    fn probe_devices_does_not_fail() {
        let listing = probe_devices().expect("列挙できる");
        for d in &listing.devices {
            println!("{:?} {:?} {:?} driver={:?}", d.direction, d.name, d.id, d.driver);
        }
    }
}
