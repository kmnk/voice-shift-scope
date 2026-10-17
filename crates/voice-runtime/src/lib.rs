//! 音声の実行（cpal での機器の列挙、音声のスレッド、分析のスレッド）。
//! 今は機器の列挙だけを持つ。

mod devices;

pub use devices::{DeviceDirection, DeviceListing, ProbedDevice, probe_devices};
