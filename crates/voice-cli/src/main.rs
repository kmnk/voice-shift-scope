//! 開発で確かめるための CLI。アプリの機能ではない。

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
    about = "voice-shift-scope の中核を確かめる CLI。機器を開かず、ファイルの読み書きと機器の列挙だけを行う"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 録音と再生の機器を列挙し、JSON で出す（機器は開かない）
    Devices,
    /// wav を VB-CABLE の再生側に流して録音側から録り返し、遅れと強さを JSON で出す。
    /// 声を変える処理は通さない。VB-CABLE が無ければ終了コード 2
    Loopback {
        /// 流す wav（mono）
        #[arg(long)]
        input_wav: PathBuf,
        /// 流す前に置く無音（秒）。遅れを測る起点にする
        #[arg(long, default_value_t = 0.5)]
        lead: f64,
        /// 録り返した音を書く先
        #[arg(long)]
        out_wav: Option<PathBuf>,
    },
    /// wav を分析し、10 ms ごとの F0・F1〜F4・倍音を CSV で標準出力に書く（開発で確かめるための道具）
    Analyze {
        wav: PathBuf,
        /// フォルマントの上限（Hz）
        #[arg(long, default_value_t = 5500.0)]
        ceiling: f32,
        /// LPC の次数（フォルマントの数の 2 倍）
        #[arg(long, default_value_t = 10)]
        lpc_order: u32,
    },
    /// 受け入れの基準を試験用の信号で判定する。FAIL があれば終了コード 1
    Check {
        /// 判定する基準（カンマ区切り。今あるのは A1〜A4・B1・B3・Q2〜Q5）
        #[arg(long, value_delimiter = ',', default_value = "B1,B3")]
        criteria: Vec<String>,
        #[arg(long, default_value_t = 5500.0)]
        ceiling: f32,
        #[arg(long, default_value_t = 10)]
        lpc_order: u32,
        /// 声を変える処理の窓の長さ（ms）。省くと既定（70 ms）
        #[arg(long)]
        block_ms: Option<f32>,
        /// 判定に使う声を変える処理。app（アプリと同じ）か signalsmith（変更の前の、Signalsmith Stretch だけ）
        #[arg(long, default_value = "app", value_parser = ["app", "signalsmith"])]
        shifter: String,
        /// 結果を JSON で出す
        #[arg(long)]
        json: bool,
    },
    /// 音声の実行を、入力だけ wav（省くと F0 150 Hz の合成の母音）に替えて本物の機器で通し、
    /// VB-CABLE から録り返して A1・A5・A6 を判定する。VB-CABLE が無ければ終了コード 2
    E2e {
        #[arg(long)]
        input_wav: Option<PathBuf>,
        /// 動かす長さ（秒）
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
        /// ピッチの変化（半音）
        #[arg(long, default_value_t = 5.0, allow_hyphen_values = true)]
        semitones: f32,
    },
    /// wav（48 kHz、mono）を、アプリと同じ声を変える処理に 10 ms ずつ通し、遅れの分を前から除いて書く。
    /// 機器は開かない
    Shift {
        input: PathBuf,
        output: PathBuf,
        /// ピッチの変化（半音、−24〜+24）
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        pitch: f32,
        /// フォルマントの倍率（0.5〜2.0）
        #[arg(long, default_value_t = 1.0)]
        formant: f32,
        /// 声を変える処理。app（アプリと同じ）か signalsmith（変更の前の、Signalsmith Stretch だけ）
        #[arg(long, default_value = "app", value_parser = ["app", "signalsmith"])]
        shifter: String,
        /// 1 ブロック（10 ms）の処理時間の中央値と最大を、JSON で標準出力に書く
        #[arg(long)]
        timing: bool,
        /// F0 の目安を渡さない。省くと、アプリの分析のスレッドと同じく、直近 4096 標本の F0（YIN）を
        /// 10 ms ごとに声を変える処理に渡す（0 のときは渡さず、前の値が残る）
        #[arg(long)]
        no_hint: bool,
    },
    /// 答えの分かる試験用の信号を wav（48 kHz、mono、32-bit float）で書く
    Synth {
        #[command(subcommand)]
        signal: Signal,
    },
}

#[derive(Subcommand)]
enum Signal {
    /// spec の合成の母音（F1 700・F2 1200・F3 2600・F4 3500 Hz に F5 4500 Hz を足したもの）
    Vowel {
        /// 基本周波数（Hz）
        #[arg(long)]
        f0: f64,
        /// 長さ（秒）
        #[arg(long, default_value_t = 2.0)]
        seconds: f64,
        /// 書く先
        #[arg(long)]
        out: PathBuf,
    },
    /// H1〜Hn を同じ強さで並べ、雑音を足した信号
    Harmonics {
        #[arg(long)]
        f0: f64,
        /// 倍音の数
        #[arg(long, default_value_t = 8)]
        count: usize,
        /// 倍音より何 dB 低い雑音を足すか
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
        Command::Analyze { wav, ceiling, lpc_order } => run_analyze(&wav, ceiling, lpc_order),
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
                    eprintln!("{} に書けませんでした: {e}", out.display());
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
            eprintln!("機器を列挙できませんでした: {e}");
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
    println!("{}", serde_json::to_string_pretty(&out).expect("JSON にできる"));
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
            eprintln!("{} を読めませんでした: {e}", input_wav.display());
            return ExitCode::FAILURE;
        }
    };
    let listing = match probe_devices() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("機器を列挙できませんでした: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(cable) = find_virtual_cable(&listing.devices) else {
        let out = json!({ "virtual_cable": null, "error": "VB-CABLE が見つかりません。VB-CABLE を入れてから動かしてください" });
        println!("{}", serde_json::to_string_pretty(&out).expect("JSON にできる"));
        return ExitCode::from(2);
    };

    let mut played = vec![0.0f32; (lead * rate as f64) as usize];
    played.extend_from_slice(&signal);
    let recording = match loopback(&played, rate, &cable.playback.id, &cable.recording.id, std::time::Duration::from_secs(1)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("loopback に失敗しました: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(path) = out_wav {
        if let Err(e) = write_wav(path, &recording.samples) {
            eprintln!("{} に書けませんでした: {e}", path.display());
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
    println!("{}", serde_json::to_string_pretty(&out).expect("JSON にできる"));
    if recorded_onset.is_some() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn analysis_settings(ceiling: f32, lpc_order: u32) -> Result<voice_domain::AnalysisSettings, voice_domain::DomainError> {
    Ok(voice_domain::AnalysisSettings {
        ceiling: voice_domain::FormantCeiling::new(ceiling)?,
        lpc_order: voice_domain::LpcOrder::new(lpc_order)?,
        ..Default::default()
    })
}

fn run_analyze(wav: &std::path::Path, ceiling: f32, lpc_order: u32) -> ExitCode {
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
            eprintln!("{} を読めませんでした: {e}", wav.display());
            return ExitCode::FAILURE;
        }
    };
    println!("{}", analysis::csv_header());
    for (t, f) in analysis::analyze_signal(&x, rate, &settings) {
        println!("{}", analysis::csv_row(t, &f));
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
        println!("{}", serde_json::to_string_pretty(&v).expect("JSON にできる"));
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
                eprintln!("{} は {rate} Hz です。{} Hz の wav を渡してください", path.display(), check::RATE);
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("{} を読めませんでした: {e}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => (synth::vowel(150.0, &synth::SYNTH_VOWEL, check::RATE as f64, 2.0, 0.5), Some(150.0)),
    };
    match e2e::run(&signal, input_f0, semitones, seconds) {
        Ok(o) => {
            println!("{}", serde_json::to_string_pretty(&o.report).expect("JSON にできる"));
            if o.no_cable {
                ExitCode::from(2)
            } else if o.pass {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("e2e に失敗しました: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_shift(input: &std::path::Path, output: &std::path::Path, pitch: f32, formant: f32, timing: bool, hint: bool) -> ExitCode {
    let (x, rate) = match read_wav(input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} を読めませんでした: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    if rate != check::RATE {
        eprintln!("{} は {rate} Hz です。{} Hz の wav を渡してください", input.display(), check::RATE);
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
    // アプリの分析のスレッドと同じく、直近 4096 標本の F0 を目安として渡す（音声のスレッドと同じく、0 と同じ値は渡さない）
    use voice_domain::ports::Analyzer;
    let mut analyzer = voice_dsp::DefaultAnalyzer::default();
    let analysis = voice_domain::AnalysisSettings::default();
    let sample_rate = voice_domain::SampleRate::new(check::RATE).expect("0 より大きい");
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
        eprintln!("{} に書けませんでした: {e}", output.display());
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
