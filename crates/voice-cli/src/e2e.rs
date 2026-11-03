//! Runs the audio runtime (voice-runtime) on real devices, with only the input replaced by a wav source.
//! The output goes to the VB-CABLE playback side and is recorded back from the recording side for the checks.

use std::sync::{Arc, Mutex};
use std::time::Duration;


use serde_json::{Value, json};
use voice_domain::ports::{AnalysisPublisher, AudioDevices, AudioPipeline, PipelineConfig};
use voice_domain::{AnalysisPair, DeviceDirection, FormantShift, MonitorSettings, MonitorVolume, PitchShift, ShiftSettings};
use voice_dsp::DefaultAnalyzer;
use voice_runtime::loopback::{Recorder, onset, resample_linear};
use voice_runtime::{CpalDevices, RuntimePipeline};

use crate::analysis::{analyze_signal, median};
use crate::check::RATE;

#[derive(Default)]
struct Collect {
    before_f0: Mutex<Vec<f32>>,
    after_f0: Mutex<Vec<f32>>,
}

impl AnalysisPublisher for Collect {
    fn publish(&self, pair: &AnalysisPair) {
        if let Some(f) = pair.before.f0 {
            self.before_f0.lock().expect("collector").push(f.hz());
        }
        if let Some(f) = pair.after.f0 {
            self.after_f0.lock().expect("collector").push(f.hz());
        }
    }
}

pub struct E2eOutcome {
    pub report: Value,
    pub pass: bool,
    pub no_cable: bool,
}

/// Plays `signal` (`RATE`, mono) and checks that the voice shifted by `semitones` can be recorded from VB-CABLE.
pub fn run(signal: &[f32], input_f0: Option<f32>, semitones: f32, seconds: f64) -> Result<E2eOutcome, String> {
    let devices = CpalDevices;
    let list = devices.list().map_err(|e| e.to_string())?;
    let Some(cable) = list.cable().cloned() else {
        return Ok(E2eOutcome {
            report: json!({ "virtual_cable": null, "error": "VB-CABLE not found. Install VB-CABLE before running this" }),
            pass: false,
            no_cable: true,
        });
    };

    // The signal: 0.5 s of silence, then repeated up to `seconds` seconds
    let lead = (0.5 * RATE as f64) as usize;
    let total = lead + (seconds * RATE as f64) as usize;
    let mut played = vec![0.0f32; lead];
    while played.len() < total {
        let n = (total - played.len()).min(signal.len());
        played.extend_from_slice(&signal[..n]);
    }

    let collect = Arc::new(Collect::default());
    let mut pipeline = RuntimePipeline::new(
        Box::new(voice_dsp::app_shifter),
        Box::new(|| Box::new(DefaultAnalyzer::default())),
        collect.clone(),
    )
    .with_input_samples(played.clone());
    let shift = ShiftSettings {
        pitch: PitchShift::new(semitones).map_err(|e| e.to_string())?,
        formant: FormantShift::NONE,
    };
    // The input is replaced by the wav source, so the input device value is not used. CI runners have
    // no recording device other than VB-CABLE, which cannot satisfy the use case rule (the VB-CABLE recording side cannot
    // be the input), so the port is called directly here instead of going through the use case
    let placeholder_input = list.id(cable.recording.raw(), DeviceDirection::Input).map_err(|e| e.to_string())?;
    let config = PipelineConfig {
        input: placeholder_input,
        output: cable.playback.clone(),
        monitor: MonitorSettings { enabled: false, device: None, volume: MonitorVolume::DEFAULT },
        routing: Default::default(),
        shift,
        analysis: Default::default(),
    };

    let recorder = Recorder::start(cable.recording.raw()).map_err(|e| e.to_string())?;
    pipeline.start(config).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs_f64(seconds + 1.5));
    let stats = pipeline.stats();
    let source_started = pipeline.source_started();
    let recording_started = recorder.first_sample_at();
    pipeline.stop().map_err(|e| e.to_string())?;
    let (recorded, rec_rate) = recorder.stop();
    let recorded = resample_linear(&recorded, rec_rate, RATE);

    let peak = signal.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // Recording and the source start separately, so add the difference between their start times
    let start_offset_ms = match (source_started, recording_started) {
        (Some(src), Some(rec)) => Some(if rec >= src {
            (rec - src).as_secs_f64() * 1000.0
        } else {
            -((src - rec).as_secs_f64() * 1000.0)
        }),
        _ => None,
    };
    let latency_ms = match (onset(&played, peak * 0.1), onset(&recorded, peak * 0.1), start_offset_ms) {
        (Some(s), Some(r), Some(offset)) => Some((r as f64 - s as f64) * 1000.0 / RATE as f64 + offset),
        _ => None,
    };
    let frames = analyze_signal(&recorded, RATE, &Default::default());
    let start_t = latency_ms.unwrap_or(0.0) / 1000.0 + 1.0;
    let recorded_f0 = median(frames.iter().filter(|(t, _)| *t >= start_t).filter_map(|(_, f)| f.f0.map(|v| v.hz())).collect());
    let expected_f0 = input_f0.map(|f| f * 2f32.powf(semitones / 12.0));
    let f0_diff = recorded_f0.zip(expected_f0).map(|(r, e)| (r / e - 1.0) * 100.0);
    let before_f0 = median(collect.before_f0.lock().expect("collector").clone());
    let after_f0 = median(collect.after_f0.lock().expect("collector").clone());

    let a1 = f0_diff.is_some_and(|d| d.abs() <= 1.0);
    // The delay can be measured from the waveform onset only at 0 semitones, where the shifter is nearly transparent
    let a5 = latency_ms.is_some_and(|l| l <= 150.0);
    let measure_latency = semitones == 0.0;
    let a6 = stats.output_underruns == 0 && stats.dropped_input == 0;
    let analysis_ok = stats.published_pairs as f64 >= seconds * 80.0;
    let report = json!({
        "virtual_cable": { "playback": cable.playback.raw(), "recording": cable.recording.raw() },
        "semitones": semitones,
        "seconds": seconds,
        "recording_rate": rec_rate,
        "A1": { "result": pass_str(a1), "input_f0": input_f0, "expected_f0": expected_f0, "recorded_f0": recorded_f0, "diff_percent": f0_diff },
        "A5": if measure_latency { json!({ "result": pass_str(a5), "latency_ms": latency_ms, "recording_minus_source_start_ms": start_offset_ms, "limit_ms": 150, "note": "From the wav source to CABLE Output. Does not include the microphone input buffer" }) } else { json!({ "result": "SKIP", "note": "Measured only with --semitones 0" }) },
        "A6": { "result": pass_str(a6), "output_underruns": stats.output_underruns, "output_underrun_events": stats.output_underrun_events, "first_underrun_seconds": stats.first_underrun_seconds, "last_underrun_seconds": stats.last_underrun_seconds, "dropped_input": stats.dropped_input },
        "analysis": { "result": pass_str(analysis_ok), "published_pairs": stats.published_pairs, "before_f0_median": before_f0, "after_f0_median": after_f0 },
        "shifter_latency_samples": stats.shifter_latency,
        "output_fill_ms": stats.output_fill_ms,
        "output_correction_ppm": stats.output_correction_ppm,
        "processed_samples": stats.processed_samples,
        "errors": stats.errors,
    });
    Ok(E2eOutcome { report, pass: a1 && (a5 || !measure_latency) && a6 && analysis_ok && stats.errors.is_empty(), no_cable: false })
}

fn pass_str(ok: bool) -> &'static str {
    if ok { "PASS" } else { "FAIL" }
}
