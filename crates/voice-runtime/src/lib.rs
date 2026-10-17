//! 音声の実行（cpal での機器の列挙、音声のスレッド、分析のスレッド）。
//! 今は機器の列挙、VB-CABLE を見つける処理、試験の道具の loopback を持つ。

mod devices;
pub mod loopback;
mod virtual_cable;

pub use devices::{DeviceDirection, DeviceListing, ProbedDevice, probe_devices};
pub use virtual_cable::{VB_CABLE_DRIVER, VirtualCable, find_virtual_cable};
