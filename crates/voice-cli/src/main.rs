//! 開発で確かめるための CLI。アプリの機能ではない。

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
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Devices => devices(),
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
