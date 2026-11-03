//! A CLI for checks during development. Not a feature of the app.

mod analysis;
mod check;
mod e2e;
mod synth;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::json;
use voice_runtime::loopback::{loopback, onset};
use voice_runtime::{DeviceDirection, find_virtual_cable, probe_devices};

#[derive(Parser)]
#[command(
    name = "voice-cli",
    about = "A CLI for checking the core of voice-shift-scope. It does not open devices; it only reads and writes files and lists devices"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the recording and playback devices as JSON (does not open them)
    Devices,
    /// Play a wav into the VB-CABLE playback side, record it back from the recording side, and print the delay and level as JSON.
    /// Does not go through the shifter. Exit code 2 without VB-CABLE
    Loopback {
        /// The wav to play (mono)
        #[arg(long)]
        input_wav: PathBuf,
        /// Silence before playing (seconds). The starting point for measuring the delay
        #[arg(long, default_value_t = 0.5)]
        lead: f64,
        /// Where to write the recording
        #[arg(long)]
        out_wav: Option<PathBuf>,
    },
    /// Analyse a wav and write F0, F1–F4, and harmonics every 10 ms as CSV to standard output (a tool for checks during development)
    Analyze {
        wav: PathBuf,
        /// Formant ceiling (Hz)
        #[arg(long, default_value_t = 5500.0)]
        ceiling: f32,
        /// LPC order (twice the number of formants)
        #[arg(long, default_value_t = 10)]
        lpc_order: u32,
        /// Write the 4 lowest roots of each frame without tracking F1–F4 (to compare before and after tracking)
        #[arg(long)]
        raw_formants: bool,
        /// Write the median and maximum processing time of one analysis (ms) as JSON to standard error
        #[arg(long)]
        timing: bool,
    },
    /// Run the acceptance checks on test signals. Exit code 1 if any FAIL
    Check {
        /// Checks to run (comma-separated; available: A1–A4, B1, B3, Q2–Q5)
        #[arg(long, value_delimiter = ',', default_value = "B1,B3")]
        criteria: Vec<String>,
        #[arg(long, default_value_t = 5500.0)]
        ceiling: f32,
        #[arg(long, default_value_t = 10)]
        lpc_order: u32,
        /// Window length of the shifter (ms). Defaults to 70 ms
        #[arg(long)]
        block_ms: Option<f32>,
        /// The shifter for the checks: app (as in the app) or signalsmith (Signalsmith Stretch only, as before the change)
        #[arg(long, default_value = "app", value_parser = ["app", "signalsmith"])]
        shifter: String,
        /// Print the results as JSON
        #[arg(long)]
        json: bool,
    },
    /// Run the audio runtime on real devices with only the input replaced by a wav (a synthetic vowel at F0 150 Hz if omitted),
    /// record it back from VB-CABLE, and check A1, A5, and A6. Exit code 2 without VB-CABLE
    E2e {
        #[arg(long)]
        input_wav: Option<PathBuf>,
        /// How long to run (seconds)
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
        /// Pitch change (semitones)
        #[arg(long, default_value_t = 5.0, allow_hyphen_values = true)]
        semitones: f32,
    },
    /// Run a wav (48 kHz, mono) through the same shifter as the app in 10 ms steps, and write it with the delay removed from the front.
    /// Does not open devices
    Shift {
        input: PathBuf,
        output: PathBuf,
        /// Pitch change (semitones, −24 to +24)
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        pitch: f32,
        /// Formant factor (0.5 to 2.0)
        #[arg(long, default_value_t = 1.0)]
        formant: f32,
        /// The shifter: app (as in the app) or signalsmith (Signalsmith Stretch only, as before the change)
        #[arg(long, default_value = "app", value_parser = ["app", "signalsmith"])]
        shifter: String,
        /// Write the median and maximum processing time of one block (10 ms) as JSON to standard output
        #[arg(long)]
        timing: bool,
        /// Do not give F0 hints. Without this flag, as in the app's analysis thread, the F0 (YIN) of the latest 4096 samples is
        /// given to the shifter every 10 ms (nothing is given for 0, and the previous value remains)
        #[arg(long)]
        no_hint: bool,
    },
    /// Write test signals with known answers as wav (48 kHz, mono, 32-bit float)
    Synth {
        #[command(subcommand)]
        signal: Signal,
    },
}

#[derive(Subcommand)]
enum Signal {
    /// The synthetic test vowel ("Test signals" in docs/criteria.md: F1 700, F2 1200, F3 2600, F4 3500 Hz plus F5 4500 Hz)
    Vowel {
        /// Fundamental frequency (Hz)
        #[arg(long)]
        f0: f64,
        /// Length (seconds)
        #[arg(long, default_value_t = 2.0)]
        seconds: f64,
        /// Where to write
        #[arg(long)]
        out: PathBuf,
    },
    /// H1–Hn at equal strength, plus noise
    Harmonics {
        #[arg(long)]
        f0: f64,
        /// Number of harmonics
        #[arg(long, default_value_t = 8)]
        count: usize,
        /// How many dB below the harmonics the added noise is
        #[arg(long, default_value_t = 40.0)]
        noise_below_db: f64,
        #[arg(long, default_value_t = 2.0)]
        seconds: f64,
        #[arg(long)]
        out: PathBuf,
    },
}

const SAMPLE_RATE: u32 = 48_000;

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Devices => devices(),
        Command::E2e { input_wav, seconds, semitones } => run_e2e(input_wav.as_deref(), seconds, semitones),
        Command::Analyze { wav, ceiling, lpc_order, raw_formants, timing } => {
            run_analyze(&wav, ceiling, lpc_order, raw_formants, timing)
        }
        Command::Check { criteria, ceiling, lpc_order, block_ms, shifter, json } => {
            if let Some(ms) = block_ms {
                let _ = check::BLOCK_SECONDS.set(ms / 1000.0);
            }
            let _ = check::SHIFTER.set(shifter);
            run_check(&criteria, ceiling, lpc_order, json)
        }
        Command::Shift { input, output, pitch, formant, shifter, timing, no_hint } => {
            let _ = check::SHIFTER.set(shifter);
            run_shift(&input, &output, pitch, formant, timing, !no_hint)
        }
        Command::Loopback { input_wav, lead, out_wav } => run_loopback(&input_wav, lead, out_wav.as_deref()),
        Command::Synth { signal } => {
            let (samples, out) = match signal {
                Signal::Vowel { f0, seconds, out } => (
                    synth::vowel(f0, &synth::SYNTH_VOWEL, SAMPLE_RATE as f64, seconds, 0.5),
                    out,
                ),
                Signal::Harmonics { f0, count, noise_below_db, seconds, out } => (
                    synth::harmonics(f0, count, noise_below_db, SAMPLE_RATE as f64, seconds, 0.5),
                    out,
                ),
            };
            match write_wav(&out, &samples) {
                Ok(()) => {
                    println!("{}", out.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("could not write {}: {e}", out.display());
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn devices() -> ExitCode {
    let listing = match probe_devices() {
        Ok(listing) => listing,
        Err(e) => {
            eprintln!("could not enumerate the devices: {e}");
            return ExitCode::FAILURE;
        }
    };
    let devices: Vec<_> = listing
        .devices
        .iter()
        .map(|d| {
            json!({
                "direction": match d.direction {
                    DeviceDirection::Input => "input",
                    DeviceDirection::Output => "output",
                },
                "id": d.id,
                "name": d.name,
                "manufacturer": d.manufacturer,
                "driver": d.driver,
                "extended": d.extended,
                "is_default": d.is_default,
            })
        })
        .collect();
    let out = json!({ "host": listing.host, "devices": devices });
    println!("{}", serde_json::to_string_pretty(&out).expect("serializable as JSON"));
    ExitCode::SUCCESS
}

fn write_wav(path: &std::path::Path, samples: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for s in samples {
        writer.write_sample(*s)?;
    }
    writer.finalize()
}

fn read_wav(path: &std::path::Path) -> Result<(Vec<f32>, u32), hound::Error> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let mono = samples.chunks(channels).map(|f| f[0]).collect();
    Ok((mono, spec.sample_rate))
}

fn run_loopback(input_wav: &std::path::Path, lead: f64, out_wav: Option<&std::path::Path>) -> ExitCode {
    let (signal, rate) = match read_wav(input_wav) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not read {}: {e}", input_wav.display());
            return ExitCode::FAILURE;
        }
    };
    let listing = match probe_devices() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not enumerate the devices: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(cable) = find_virtual_cable(&listing.devices) else {
        let out = json!({ "virtual_cable": null, "error": "VB-CABLE not found. Install VB-CABLE before running this" });
        println!("{}", serde_json::to_string_pretty(&out).expect("serializable as JSON"));
        return ExitCode::from(2);
    };

    let mut played = vec![0.0f32; (lead * rate as f64) as usize];
    played.extend_from_slice(&signal);
    let recording = match loopback(&played, rate, &cable.playback.id, &cable.recording.id, std::time::Duration::from_secs(1)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("loopback failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(path) = out_wav {
        if let Err(e) = write_wav(path, &recording.samples) {
            eprintln!("could not write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }

    let peak = signal.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let threshold = peak * 0.1;
    let source_onset = onset(&played, threshold);
    let recorded_onset = onset(&recording.samples, threshold);
    let latency_ms = match (source_onset, recorded_onset) {
        (Some(s), Some(r)) => Some((r as f64 - s as f64) * 1000.0 / rate as f64),
        _ => None,
    };
    let rms = |x: &[f32]| (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt();
    let level_db = match (source_onset, recorded_onset) {
        (Some(s), Some(r)) => {
            let n = (played.len() - s).min(recording.samples.len().saturating_sub(r));
            Some(20.0 * (rms(&recording.samples[r..r + n]) / rms(&played[s..s + n])).log10())
        }
        _ => None,
    };
    let out = json!({
        "virtual_cable": {
            "playback": { "name": cable.playback.name, "driver": cable.playback.driver },
            "recording": { "name": cable.recording.name, "driver": cable.recording.driver },
        },
        "sample_rate": rate,
        "output_device_rate": recording.output_device_rate,
        "input_device_rate": recording.input_device_rate,
        "output_channels": recording.output_channels,
        "input_channels": recording.input_channels,
        "recorded_seconds": recording.samples.len() as f64 / rate as f64,
        "signal_detected": recorded_onset.is_some(),
        "latency_ms": latency_ms,
        "level_db": level_db,
    });
    println!("{}", serde_json::to_string_pretty(&out).expect("serializable as JSON"));
    if recorded_onset.is_some() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn analysis_settings(ceiling: f32, lpc_order: u32) -> Result<voice_domain::AnalysisSettings, voice_domain::DomainError> {
    Ok(voice_domain::AnalysisSettings {
        ceiling: voice_domain::FormantCeiling::new(ceiling)?,
        lpc_order: voice_domain::LpcOrder::new(lpc_order)?,
        ..Default::default()
    })
}

fn run_analyze(wav: &std::path::Path, ceiling: f32, lpc_order: u32, raw_formants: bool, timing: bool) -> ExitCode {
    let settings = match analysis_settings(ceiling, lpc_order) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let (x, rate) = match read_wav(wav) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not read {}: {e}", wav.display());
            return ExitCode::FAILURE;
        }
    };
    let mut analyzer = voice_dsp::DefaultAnalyzer { raw_formants, ..Default::default() };
    let mut times = Vec::new();
    println!("{}", analysis::csv_header());
    for (t, f) in analysis::analyze_signal_with(&mut analyzer, &x, rate, &settings, timing.then_some(&mut times)) {
        println!("{}", analysis::csv_row(t, &f));
    }
    if timing {
        times.sort_by(f64::total_cmp);
        let median = times.get(times.len() / 2).copied().unwrap_or(0.0);
        let max = times.last().copied().unwrap_or(0.0);
        eprintln!("{}", serde_json::json!({ "frames": times.len(), "median_ms": median, "max_ms": max }));
    }
    ExitCode::SUCCESS
}

fn run_check(criteria: &[String], ceiling: f32, lpc_order: u32, as_json: bool) -> ExitCode {
    let settings = match analysis_settings(ceiling, lpc_order) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let outcomes = match check::run(criteria, &settings) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if as_json {
        let v: Vec<_> = outcomes
            .iter()
            .map(|o| json!({ "criterion": o.criterion, "case": o.case, "result": if o.pass { "PASS" } else { "FAIL" }, "detail": o.detail }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&v).expect("serializable as JSON"));
    } else {
        for o in &outcomes {
            println!("{}\t{}\t{}\t{}", o.criterion, o.case, if o.pass { "PASS" } else { "FAIL" }, o.detail);
        }
    }
    if outcomes.iter().all(|o| o.pass) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn run_e2e(input_wav: Option<&std::path::Path>, seconds: f64, semitones: f32) -> ExitCode {
    let (signal, input_f0) = match input_wav {
        Some(path) => match read_wav(path) {
            Ok((x, rate)) if rate == check::RATE => (x, None),
            Ok((_, rate)) => {
                eprintln!("{} is {rate} Hz. Give a {} Hz wav", path.display(), check::RATE);
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("could not read {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => (synth::vowel(150.0, &synth::SYNTH_VOWEL, check::RATE as f64, 2.0, 0.5), Some(150.0)),
    };
    match e2e::run(&signal, input_f0, semitones, seconds) {
        Ok(o) => {
            println!("{}", serde_json::to_string_pretty(&o.report).expect("serializable as JSON"));
            if o.no_cable {
                ExitCode::from(2)
            } else if o.pass {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("e2e failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_shift(input: &std::path::Path, output: &std::path::Path, pitch: f32, formant: f32, timing: bool, hint: bool) -> ExitCode {
    let (x, rate) = match read_wav(input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not read {}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    if rate != check::RATE {
        eprintln!("{} is {rate} Hz. Give a {} Hz wav", input.display(), check::RATE);
        return ExitCode::FAILURE;
    }
    let settings = match (voice_domain::PitchShift::new(pitch), voice_domain::FormantShift::new(formant)) {
        (Ok(pitch), Ok(formant)) => voice_domain::ShiftSettings { pitch, formant },
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut shifter = check::make_shifter();
    shifter.set(settings);
    let latency = shifter.latency();
    let block = 480;
    let mut padded = x.clone();
    padded.resize(x.len() + latency + block, 0.0);
    let mut out = vec![0.0f32; padded.len()];
    let mut times = Vec::with_capacity(padded.len() / block + 1);
    // As in the app's analysis thread, give the F0 of the latest 4096 samples as a hint (as on the audio thread, 0 and unchanged values are not given)
    use voice_domain::ports::Analyzer;
    let mut analyzer = voice_dsp::DefaultAnalyzer::default();
    let analysis = voice_domain::AnalysisSettings::default();
    let sample_rate = voice_domain::SampleRate::new(check::RATE).expect("greater than 0");
    let window = 4096usize;
    let mut last_hint = 0.0f32;
    for (k, (i, o)) in padded.chunks(block).zip(out.chunks_mut(block)).enumerate() {
        let end = (k * block).min(x.len());
        if hint && end >= window {
            let f0 = analyzer.pitch_only(&x[end - window..end], sample_rate, &analysis).map(|f| f.hz()).unwrap_or(0.0);
            if f0 > 0.0 && f0 != last_hint {
                shifter.set_pitch_hint(f0);
                last_hint = f0;
            }
        }
        let t = std::time::Instant::now();
        shifter.process(i, o);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    out.drain(..latency);
    out.truncate(x.len());
    if let Err(e) = write_wav(output, &out) {
        eprintln!("could not write {}: {e}", output.display());
        return ExitCode::FAILURE;
    }
    if timing {
        times.sort_by(f64::total_cmp);
        let median = times[times.len() / 2];
        let max = times[times.len() - 1];
        println!(
            "{}",
            serde_json::json!({ "blocks": times.len(), "block_samples": block, "median_ms": median, "max_ms": max, "latency_samples": latency })
        );
    }
    ExitCode::SUCCESS
}
