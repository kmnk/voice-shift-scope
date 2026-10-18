//! 入る側の adapter と組み立て（composition root）。Tauri に依るのはこの crate だけ。
//! command は、画面から来た値を value object に直して use case を 1 つ呼ぶだけにする。

mod dto;
mod preferences;
mod publisher;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use voice_app::{AppError, VoiceStudio};
use voice_domain::{
    AnalysisSettings, DomainError, FormantCeiling, FormantShift, HarmonicThreshold, LpcOrder, MonitorVolume,
    PitchShift, ShiftSettings,
};
use voice_dsp::{DefaultAnalyzer, SignalsmithShifter};
use voice_runtime::{CpalDevices, RuntimePipeline};

use crate::dto::{DevicesDto, PairDto, devices_dto};
use crate::preferences::PreferencesStore;
use crate::publisher::ChannelPublisher;

type Studio = VoiceStudio<TestSourceDevices, RuntimePipeline>;

/// `--input-wav` のときだけ、一覧の入力に「試験の音源」を足す `AudioDevices`。
/// 音声の実行は入力の機器の代わりに wav を流すので、この機器の値は使われない。
/// use case の規則（入力を選ばないと開始できない）はそのまま通す。
struct TestSourceDevices {
    inner: CpalDevices,
    test_source: bool,
}

const TEST_SOURCE_ID: &str = "test:input-wav";

impl voice_domain::ports::AudioDevices for TestSourceDevices {
    type Error = voice_runtime::RuntimeError;

    fn list(&self) -> Result<voice_domain::DeviceList, Self::Error> {
        let list = self.inner.list()?;
        if !self.test_source {
            return Ok(list);
        }
        Ok(list.with_extra_input(voice_domain::DeviceInfo {
            raw_id: TEST_SOURCE_ID.into(),
            name: "試験の音源（--input-wav）".into(),
            direction: voice_domain::DeviceDirection::Input,
            is_default: true,
        }))
    }
}

struct AppState {
    studio: Mutex<Studio>,
    publisher: Arc<ChannelPublisher>,
    preferences: PreferencesStore,
    /// `--ui-selftest <dir>` のときの結果の置き場。試験のための入口で、画面からは選べない。
    selftest_dir: Option<PathBuf>,
}

/// 画面に返す誤り。
#[derive(Debug, Serialize)]
pub struct CommandError {
    kind: &'static str,
    message: String,
}

impl From<AppError> for CommandError {
    fn from(e: AppError) -> Self {
        let kind = match &e {
            AppError::Domain(_) => "rule",
            AppError::Devices(_) => "devices",
            AppError::Pipeline(_) => "audio",
        };
        Self { kind, message: e.to_string() }
    }
}

impl From<DomainError> for CommandError {
    fn from(e: DomainError) -> Self {
        Self { kind: "invalid", message: e.to_string() }
    }
}

fn io_error(e: impl std::fmt::Display) -> CommandError {
    CommandError { kind: "io", message: e.to_string() }
}

type Res<T> = Result<T, CommandError>;

fn snapshot(studio: &Studio) -> DevicesDto {
    let list = studio.devices();
    devices_dto(list, studio.session(), |raw| list.name(raw).unwrap_or(raw).to_string())
}

#[tauri::command]
fn list_devices(state: State<AppState>) -> Res<DevicesDto> {
    let mut studio = state.studio.lock().expect("studio");
    studio.list_devices()?;
    Ok(snapshot(&studio))
}

#[tauri::command]
fn select_input(state: State<AppState>, id: String) -> Res<DevicesDto> {
    let mut studio = state.studio.lock().expect("studio");
    studio.select_input(&id)?;
    Ok(snapshot(&studio))
}

#[tauri::command]
fn set_monitor(state: State<AppState>, enabled: bool, id: Option<String>) -> Res<DevicesDto> {
    let mut studio = state.studio.lock().expect("studio");
    studio.set_monitor(enabled, id.as_deref())?;
    Ok(snapshot(&studio))
}

#[tauri::command]
fn set_monitor_volume(state: State<AppState>, volume: f32) -> Res<()> {
    let volume = MonitorVolume::new(volume)?;
    Ok(state.studio.lock().expect("studio").set_monitor_volume(volume)?)
}

#[tauri::command]
fn start(state: State<AppState>, on_analysis: Channel<PairDto>) -> Res<DevicesDto> {
    state.publisher.set(on_analysis);
    let mut studio = state.studio.lock().expect("studio");
    studio.start()?;
    Ok(snapshot(&studio))
}

#[tauri::command]
fn stop(state: State<AppState>) -> Res<DevicesDto> {
    let mut studio = state.studio.lock().expect("studio");
    studio.stop()?;
    Ok(snapshot(&studio))
}

#[tauri::command]
fn change_shift(state: State<AppState>, semitones: f32, formant: f32) -> Res<()> {
    let shift = ShiftSettings { pitch: PitchShift::new(semitones)?, formant: FormantShift::new(formant)? };
    Ok(state.studio.lock().expect("studio").change_shift(shift)?)
}

#[tauri::command]
fn change_analysis(state: State<AppState>, ceiling: f32, threshold: f32, lpc_order: u32) -> Res<()> {
    let analysis = AnalysisSettings {
        ceiling: FormantCeiling::new(ceiling)?,
        threshold: HarmonicThreshold::new(threshold)?,
        lpc_order: LpcOrder::new(lpc_order)?,
    };
    Ok(state.studio.lock().expect("studio").change_analysis(analysis)?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusDto {
    running: bool,
    output_underruns: u64,
    monitor_underruns: u64,
    errors: Vec<String>,
}

#[tauri::command]
fn status(state: State<AppState>) -> StatusDto {
    let studio = state.studio.lock().expect("studio");
    let stats = studio.pipeline().stats();
    StatusDto {
        running: studio.session().state() == voice_domain::SessionState::Running,
        output_underruns: stats.output_underruns,
        monitor_underruns: stats.monitor_underruns,
        errors: stats.errors,
    }
}

/// 止めた画面の PNG を保存する。保存先を選ぶ画面を出すので、main のスレッドを塞がないよう async にする。
/// 選ばれなかったときは null を返す。
#[tauri::command]
async fn save_png(app: tauri::AppHandle, bytes: Vec<u8>, file_name: String) -> Res<Option<String>> {
    if !file_name.ends_with(".png") || file_name.contains(['/', '\\']) {
        return Err(io_error("ファイル名が正しくありません"));
    }
    // 試験のときだけ、保存先を選ぶ画面を出さずに書く
    if let Some(dir) = std::env::var_os("VOICE_SHIFT_SCOPE_TEST_SAVE_DIR") {
        let path = PathBuf::from(dir).join(&file_name);
        std::fs::write(&path, &bytes).map_err(io_error)?;
        return Ok(Some(path.display().to_string()));
    }
    let state = app.state::<AppState>();
    let start_dir = state.preferences.image_dir().or_else(|| app.path().picture_dir().ok());
    let mut dialog = app.dialog().file().set_file_name(&file_name).add_filter("PNG", &["png"]);
    if let Some(dir) = start_dir {
        dialog = dialog.set_directory(dir);
    }
    let Some(chosen) = dialog.blocking_save_file() else { return Ok(None) };
    let path = chosen.into_path().map_err(io_error)?;
    std::fs::write(&path, &bytes).map_err(io_error)?;
    if let Some(parent) = path.parent() {
        state.preferences.remember_image_dir(parent).map_err(io_error)?;
    }
    Ok(Some(path.display().to_string()))
}

/// 画面の通しの試験: 画面の側が、利用者と同じ操作（ボタンを押す・値を入れる）を順に行い、結果を書いて終わる。
#[tauri::command]
fn selftest_config(state: State<AppState>) -> Option<String> {
    selftest_log("selftest_config が呼ばれた");
    state.selftest_dir.as_ref().map(|d| d.display().to_string())
}

#[tauri::command]
fn finish_selftest(app: tauri::AppHandle, state: State<AppState>, report: String, passed: bool) -> Res<()> {
    let Some(dir) = state.selftest_dir.clone() else { return Err(io_error("試験の入口で起動していません")) };
    std::fs::create_dir_all(&dir).map_err(io_error)?;
    std::fs::write(dir.join("ui-report.json"), report).map_err(io_error)?;
    if let Ok(mut studio) = state.studio.lock() {
        let _ = studio.stop();
    }
    app.exit(if passed { 0 } else { 1 });
    Ok(())
}

/// `--ui-selftest <dir>` のときだけ、<dir>/app-log.txt に起動の経過を書く（試験が止まったときの手掛かり）。
fn selftest_log(line: &str) {
    if let Some(dir) = arg_value("--ui-selftest") {
        use std::io::Write;
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(PathBuf::from(dir).join("app-log.txt")) {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            let _ = writeln!(f, "{t:.3} {line}");
        }
    }
}

fn arg_value(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// 開発と試験のための `--input-wav <path>`。48 kHz の wav を入力の機器の代わりに流す。画面からは選べない。
fn input_wav_from_args() -> Option<Vec<f32>> {
    let path = arg_value("--input-wav")?;
    let mut reader = hound::WavReader::open(&path).ok()?;
    let spec = reader.spec();
    if spec.sample_rate != 48_000 {
        eprintln!("--input-wav は 48 kHz の wav にしてください（{} Hz）", spec.sample_rate);
        return None;
    }
    let channels = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().filter_map(Result::ok).map(|v| v as f32 / scale).collect()
        }
    };
    Some(samples.chunks(channels).map(|f| f[0]).collect())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .on_page_load(|_webview, payload| selftest_log(&format!("page load {:?} {}", payload.event(), payload.url())))
        .setup(|app| {
            selftest_log("setup を始めた");
            let publisher = Arc::new(ChannelPublisher::default());
            let test_samples = input_wav_from_args();
            let mut pipeline = RuntimePipeline::new(
                Box::new(|rate| Box::new(SignalsmithShifter::new(rate))),
                Box::new(|| Box::new(DefaultAnalyzer::default())),
                publisher.clone(),
            );
            let test_source = test_samples.is_some();
            if let Some(samples) = test_samples {
                pipeline = pipeline.with_input_samples(samples);
            }
            let devices = TestSourceDevices { inner: CpalDevices, test_source };
            let studio = VoiceStudio::new(devices, pipeline).map_err(|e| e.to_string())?;
            let config_dir = app.path().app_config_dir()?;
            app.manage(AppState {
                studio: Mutex::new(studio),
                publisher,
                preferences: PreferencesStore::new(config_dir),
                selftest_dir: arg_value("--ui-selftest").map(PathBuf::from),
            });
            selftest_log("setup を終えた");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_devices,
            select_input,
            set_monitor,
            set_monitor_volume,
            start,
            stop,
            change_shift,
            change_analysis,
            status,
            save_png,
            selftest_config,
            finish_selftest,
        ])
        .run(tauri::generate_context!())
        .expect("アプリを起動できませんでした");
}
