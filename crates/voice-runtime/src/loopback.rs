//! A test tool: plays a signal into the VB-CABLE playback side and records it back from the recording side.
//! It does not go through the shifter. It checks that the audio path (cpal and VB-CABLE) works and measures its delay,
//! and does not follow the audio thread rules (no allocation, no locks).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// The recorded result.
#[derive(Debug, Clone)]
pub struct LoopbackRecording {
    /// The first channel of the recording side, converted to `sample_rate`.
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

/// Plays `signal` (mono, `sample_rate`) into the `playback_id` device while recording from the `recording_id` device.
/// Keeps recording for `tail` after playback ends.
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
                let mut r = recorded.lock().expect("recording buffer");
                r.extend(data.chunks(in_channels as usize).map(|frame| frame[0]));
            },
            |e| eprintln!("recording error: {e}"),
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
            |e| eprintln!("playback error: {e}"),
            None,
        )?
    };

    input_stream.play()?;
    output_stream.play()?;
    let play_time = Duration::from_secs_f64(to_play.len() as f64 / out_rate as f64);
    std::thread::sleep(play_time + tail);
    drop(output_stream);
    drop(input_stream);

    let recorded = recorded.lock().expect("recording buffer").clone();
    Ok(LoopbackRecording {
        samples: resample_linear(&recorded, in_rate, sample_rate),
        sample_rate,
        output_device_rate: out_rate,
        input_device_rate: in_rate,
        output_channels: out_channels,
        input_channels: in_channels,
    })
}

/// Linear-interpolation resampling for the test tools.
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

/// Keeps recording from a recording device. `stop` stops it and returns the recorded samples (first channel) and the device rate.
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
                let mut r = sink.lock().expect("recording buffer");
                if r.is_empty() {
                    // Take the time the first callback arrived, minus the length it delivered, as the time of the first sample
                    let frames = data.len() / channels;
                    let span = Duration::from_secs_f64(frames as f64 / rate as f64);
                    *first.lock().expect("time") = std::time::Instant::now().checked_sub(span);
                }
                r.extend(data.chunks(channels).map(|frame| frame[0]));
            },
            |e| eprintln!("recording error: {e}"),
            None,
        )?;
        stream.play()?;
        Ok(Self { stream, recorded, rate, first_callback })
    }

    /// Approximate time of the first recorded sample (from the first callback).
    pub fn first_sample_at(&self) -> Option<std::time::Instant> {
        *self.first_callback.lock().expect("time")
    }

    pub fn stop(self) -> (Vec<f32>, u32) {
        drop(self.stream);
        let samples = self.recorded.lock().expect("recording buffer").clone();
        (samples, self.rate)
    }
}

/// The first position where the amplitude exceeds `threshold`.
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
