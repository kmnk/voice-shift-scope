//! 開発で確かめるための CLI。アプリの機能ではない。

mod synth;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::json;
use voice_runtime::{DeviceDirection, probe_devices};

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
    /// 答えの分かる試験用の信号を wav（48 kHz、mono、32-bit float）で書く
    Synth {
        #[command(subcommand)]
        signal: Signal,
    },
}

#[derive(Subcommand)]
enum Signal {
    /// spec の合成の母音（F1 700・F2 1200・F3 2600・F4 3500 Hz）
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
        Command::Synth { signal } => {
            let (samples, out) = match signal {
                Signal::Vowel { f0, seconds, out } => (
                    synth::vowel(f0, &synth::SPEC_VOWEL, SAMPLE_RATE as f64, seconds, 0.5),
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
