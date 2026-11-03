//! 試験の道具: 信号を VB-CABLE の再生側に流し、録音側から録り返す。
//! 声を変える処理を通さない。音声の経路（cpal と VB-CABLE）が通るかと遅れを確かめるためのもので、
//! 音声のスレッドの決まり（確保・ロックをしない）には従わない。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// 録り返した結果。
#[derive(Debug, Clone)]
pub struct LoopbackRecording {
    /// 録音側の 1 番目の channel を、`sample_rate` に直したもの。
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub output_device_rate: u32,
    pub input_device_rate: u32,
    pub output_channels: u16,
    pub input_channels: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum LoopbackError {
    #[error("no device with id {0}")]
    DeviceNotFound(String),
    #[error("cpal error: {0}")]
    Cpal(#[from] cpal::Error),
    #[error("the default format of the device is not f32 ({0:?})")]
    UnsupportedFormat(cpal::SampleFormat),
}

fn find_device(id: &str) -> Result<cpal::Device, LoopbackError> {
    let host = cpal::default_host();
    for device in host.devices()? {
        if device.id().map(|i| i.to_string()).as_deref() == Ok(id) {
            return Ok(device);
        }
    }
    Err(LoopbackError::DeviceNotFound(id.to_string()))
}

/// `signal`（mono、`sample_rate`）を `playback_id` の機器に流しながら、`recording_id` の機器から録る。
/// 流し終えてから `tail` だけ録り続ける。
pub fn loopback(
    signal: &[f32],
    sample_rate: u32,
    playback_id: &str,
    recording_id: &str,
    tail: Duration,
) -> Result<LoopbackRecording, LoopbackError> {
    let output = find_device(playback_id)?;
    let input = find_device(recording_id)?;
    let out_config = output.default_output_config()?;
    let in_config = input.default_input_config()?;
    for format in [out_config.sample_format(), in_config.sample_format()] {
        if format != cpal::SampleFormat::F32 {
            return Err(LoopbackError::UnsupportedFormat(format));
        }
    }
    let out_rate = out_config.sample_rate();
    let in_rate = in_config.sample_rate();
    let out_channels = out_config.channels();
    let in_channels = in_config.channels();

    let to_play = Arc::new(resample_linear(signal, sample_rate, out_rate));
    let position = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));

    let input_stream = {
        let recorded = recorded.clone();
        input.build_input_stream(
            in_config.config(),
            move |data: &[f32], _| {
                let mut r = recorded.lock().expect("録音の buffer");
                r.extend(data.chunks(in_channels as usize).map(|frame| frame[0]));
            },
            |e| eprintln!("録音の誤り: {e}"),
            None,
        )?
    };
    let output_stream = {
        let to_play = to_play.clone();
        let position = position.clone();
        output.build_output_stream(
            out_config.config(),
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(out_channels as usize) {
                    let i = position.fetch_add(1, Ordering::Relaxed);
                    let v = to_play.get(i).copied().unwrap_or(0.0);
                    frame.fill(v);
                }
            },
            |e| eprintln!("再生の誤り: {e}"),
            None,
        )?
    };

    input_stream.play()?;
    output_stream.play()?;
    let play_time = Duration::from_secs_f64(to_play.len() as f64 / out_rate as f64);
    std::thread::sleep(play_time + tail);
    drop(output_stream);
    drop(input_stream);

    let recorded = recorded.lock().expect("録音の buffer").clone();
    Ok(LoopbackRecording {
        samples: resample_linear(&recorded, in_rate, sample_rate),
        sample_rate,
        output_device_rate: out_rate,
        input_device_rate: in_rate,
        output_channels: out_channels,
        input_channels: in_channels,
    })
}

/// 試験の道具のための、線形補間のリサンプリング。
pub fn resample_linear(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let n = (x.len() as u64 * to as u64 / from as u64) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 * from as f64 / to as f64;
            let k = t.floor() as usize;
            let frac = (t - k as f64) as f32;
            let a = x[k.min(x.len() - 1)];
            let b = x[(k + 1).min(x.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

/// 録音の機器から録り続ける。`stop` で止めて、録った標本（1 番目の channel）と機器の速さを返す。
pub struct Recorder {
    stream: cpal::Stream,
    recorded: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    first_callback: Arc<Mutex<Option<std::time::Instant>>>,
}

impl Recorder {
    pub fn start(recording_id: &str) -> Result<Self, LoopbackError> {
        let input = find_device(recording_id)?;
        let config = input.default_input_config()?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(LoopbackError::UnsupportedFormat(config.sample_format()));
        }
        let channels = config.channels() as usize;
        let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));
        let sink = recorded.clone();
        let first_callback = Arc::new(Mutex::new(None));
        let first = first_callback.clone();
        let rate = config.sample_rate();
        let stream = input.build_input_stream(
            config.config(),
            move |data: &[f32], _| {
                let mut r = sink.lock().expect("録音の buffer");
                if r.is_empty() {
                    // 最初のコールバックが届いた時刻から、届いた分の長さを引いたものを、最初の標本の時刻とみなす
                    let frames = data.len() / channels;
                    let span = Duration::from_secs_f64(frames as f64 / rate as f64);
                    *first.lock().expect("時刻") = std::time::Instant::now().checked_sub(span);
                }
                r.extend(data.chunks(channels).map(|frame| frame[0]));
            },
            |e| eprintln!("録音の誤り: {e}"),
            None,
        )?;
        stream.play()?;
        Ok(Self { stream, recorded, rate, first_callback })
    }

    /// 録った最初の標本のおよその時刻（最初のコールバックから求める）。
    pub fn first_sample_at(&self) -> Option<std::time::Instant> {
        *self.first_callback.lock().expect("時刻")
    }

    pub fn stop(self) -> (Vec<f32>, u32) {
        drop(self.stream);
        let samples = self.recorded.lock().expect("録音の buffer").clone();
        (samples, self.rate)
    }
}

/// 振幅が `threshold` を最初に超えた位置。
pub fn onset(x: &[f32], threshold: f32) -> Option<usize> {
    x.iter().position(|v| v.abs() > threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_duration() {
        let x = vec![0.0f32; 48_000];
        assert_eq!(resample_linear(&x, 48_000, 44_100).len(), 44_100);
    }

    #[test]
    fn onset_finds_first_loud_sample() {
        let mut x = vec![0.0f32; 100];
        x[40] = 0.3;
        assert_eq!(onset(&x, 0.1), Some(40));
        assert_eq!(onset(&x, 0.5), None);
    }
}
