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
    AnalysisSettings, DomainError, FormantCeiling, FormantShift, HarmonicRelativeLimit, HarmonicThreshold, LpcOrder, MonitorVolume,
    PitchShift, ShiftSettings,
};
use voice_dsp::{DefaultAnalyzer, SignalsmithShifter};
use voice_runtime::{CpalDevices, RuntimePipeline};

use crate::dto::{DevicesDto, PairDto, devices_dto};
use crate::preferences::{PreferencesStore, RememberedAnalysis, RememberedDevices};
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
    /// 画面が言葉を替えて出すための印（domain の誤りの名前）。無ければ message をそのまま出す
    code: Option<String>,
    message: String,
}

/// domain の誤りの名前（`InvalidPitchShift { .. }` なら `InvalidPitchShift`）。
fn error_code(e: &DomainError) -> String {
    let s = format!("{e:?}");
    s.split(|c: char| !c.is_alphanumeric()).next().unwrap_or_default().to_string()
}

impl From<AppError> for CommandError {
    fn from(e: AppError) -> Self {
        let (kind, code) = match &e {
            AppError::Domain(d) => ("rule", Some(error_code(d))),
            AppError::Devices(_) => ("devices", None),
            AppError::Pipeline(_) => ("audio", None),
        };
        Self { kind, code, message: e.to_string() }
    }
}

impl From<DomainError> for CommandError {
    fn from(e: DomainError) -> Self {
        Self { kind: "invalid", code: Some(error_code(&e)), message: e.to_string() }
    }
}

fn io_error(e: impl std::fmt::Display) -> CommandError {
    CommandError { kind: "io", code: None, message: e.to_string() }
}

type Res<T> = Result<T, CommandError>;

/// 選んでいる機器を覚える（試験の音源は覚えない）。
fn remember_devices(state: &AppState, studio: &Studio) {
    let s = studio.session();
    let keep = |d: Option<&voice_domain::DeviceId>| d.map(|d| d.raw().to_string()).filter(|id| id != TEST_SOURCE_ID);
    let _ = state.preferences.remember_devices(RememberedDevices {
        input: keep(s.input()),
        output: keep(s.output()),
        monitor: keep(s.monitor().device.as_ref()),
    });
}

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
    remember_devices(&state, &studio);
    Ok(snapshot(&studio))
}

#[tauri::command]
fn select_output(state: State<AppState>, id: String) -> Res<DevicesDto> {
    let mut studio = state.studio.lock().expect("studio");
    studio.select_output(&id)?;
    remember_devices(&state, &studio);
    Ok(snapshot(&studio))
}

#[tauri::command]
fn set_monitor(state: State<AppState>, enabled: bool, id: Option<String>) -> Res<DevicesDto> {
    let mut studio = state.studio.lock().expect("studio");
    studio.set_monitor(enabled, id.as_deref())?;
    remember_devices(&state, &studio);
    Ok(snapshot(&studio))
}

#[tauri::command]
fn set_monitor_volume(state: State<AppState>, volume: f32) -> Res<()> {
    let volume = MonitorVolume::new(volume)?;
    Ok(state.studio.lock().expect("studio").set_monitor_volume(volume)?)
}

fn parse_source(s: &str) -> Res<voice_domain::AudioSource> {
    match s {
        "shifted" => Ok(voice_domain::AudioSource::Shifted),
        "original" => Ok(voice_domain::AudioSource::Original),
        other => Err(CommandError { kind: "invalid", code: None, message: format!("流す音は shifted か original です（{other}）") }),
    }
}

#[derive(Serialize)]
struct UiPrefsDto {
    language: &'static str,
    view: &'static str,
}

/// 画面の言葉（"en" か "ja"、初めては英語）と表示（"full" か "compact"、初めては全体）。
#[tauri::command]
fn get_ui_prefs(state: State<AppState>) -> UiPrefsDto {
    UiPrefsDto { language: state.preferences.language(), view: state.preferences.view() }
}

/// 窓の大きさ（論理の px）。簡易表示は左の列だけが入る大きさにする。
const FULL_SIZE: (f64, f64) = (1280.0, 860.0);
const COMPACT_SIZE: (f64, f64) = (300.0, 640.0);

/// 表示を全体か簡易に替えて覚え、窓の大きさを合わせる。
/// 簡易表示では、画面が測った左の列の中身の高さ（`content_height`、論理の px）に窓を合わせる。
#[tauri::command]
fn set_view(app: tauri::AppHandle, state: State<AppState>, view: String, content_height: Option<f64>) -> Res<()> {
    state.preferences.remember_view(&view).map_err(io_error)?;
    state.studio.lock().expect("studio").set_analysis_display(view != "compact")?;
    apply_view(&app, &view, content_height);
    Ok(())
}

fn apply_view(app: &tauri::AppHandle, view: &str, content_height: Option<f64>) {
    if let Some(window) = app.get_webview_window("main") {
        let compact = view == "compact";
        let (w, h) = if compact {
            (COMPACT_SIZE.0, content_height.map(|h| h.clamp(320.0, 900.0)).unwrap_or(COMPACT_SIZE.1))
        } else {
            FULL_SIZE
        };
        let (min_w, min_h) = if compact { (260.0, 480.0) } else { (960.0, 640.0) };
        let _ = window.set_min_size(Some(tauri::LogicalSize::new(min_w, min_h)));
        let _ = window.unmaximize();
        let _ = window.set_size(tauri::LogicalSize::new(w, h));
    }
}

#[tauri::command]
fn set_language(state: State<AppState>, language: String) -> Res<()> {
    state.preferences.remember_language(&language).map_err(io_error)
}

/// 出力とモニターに、変える前と後のどちらの音を流すか。
#[tauri::command]
fn set_routing(state: State<AppState>, output: String, monitor: String) -> Res<DevicesDto> {
    let routing = voice_domain::Routing { output: parse_source(&output)?, monitor: parse_source(&monitor)? };
    let mut studio = state.studio.lock().expect("studio");
    studio.set_routing(routing)?;
    Ok(snapshot(&studio))
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
fn change_analysis(state: State<AppState>, ceiling: f32, threshold: f32, relative_limit: f32, lpc_order: u32) -> Res<()> {
    let analysis = AnalysisSettings {
        ceiling: FormantCeiling::new(ceiling)?,
        threshold: HarmonicThreshold::new(threshold)?,
        relative_limit: HarmonicRelativeLimit::new(relative_limit)?,
        lpc_order: LpcOrder::new(lpc_order)?,
    };
    state.studio.lock().expect("studio").change_analysis(analysis)?;
    let _ = state.preferences.remember_analysis(RememberedAnalysis { ceiling, threshold, relative_limit, lpc_order });
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusDto {
    running: bool,
    output_underruns: u64,
    monitor_underruns: u64,
    /// Windows が録音・再生の buffer を取りこぼした回数（音が途切れた可能性がある）
    input_xruns: u64,
    output_xruns: u64,
    monitor_xruns: u64,
    /// 最後に録音を取りこぼしたときの、開始からの秒数
    last_input_xrun_seconds: f64,
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
        input_xruns: stats.input_xruns,
        output_xruns: stats.output_xruns,
        monitor_xruns: stats.monitor_xruns,
        last_input_xrun_seconds: stats.last_input_xrun_seconds,
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
            let mut studio = VoiceStudio::new(devices, pipeline).map_err(|e| e.to_string())?;
            let config_dir = app.path().app_config_dir()?;
            // 前に選んだ機器と分析の設定を戻す（一覧に無い機器や範囲の外の値は、既定のまま）
            let prefs = PreferencesStore::new(config_dir.clone()).load();
            if !test_source {
                studio.restore_devices(
                    prefs.devices.input.as_deref(),
                    prefs.devices.output.as_deref(),
                    prefs.devices.monitor.as_deref(),
                );
            }
            if let Some(a) = prefs.analysis {
                if let (Ok(ceiling), Ok(threshold), Ok(relative_limit), Ok(lpc_order)) = (
                    FormantCeiling::new(a.ceiling),
                    HarmonicThreshold::new(a.threshold),
                    HarmonicRelativeLimit::new(a.relative_limit),
                    LpcOrder::new(a.lpc_order),
                ) {
                    let _ = studio.change_analysis(AnalysisSettings { ceiling, threshold, relative_limit, lpc_order });
                }
            }
            app.manage(AppState {
                studio: Mutex::new(studio),
                publisher,
                preferences: PreferencesStore::new(config_dir),
                selftest_dir: arg_value("--ui-selftest").map(PathBuf::from),
            });
            let view = app.state::<AppState>().preferences.view();
            if view == "compact" {
                let _ = app.state::<AppState>().studio.lock().expect("studio").set_analysis_display(false);
                apply_view(app.handle(), view, None);
            }
            selftest_log("setup を終えた");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_devices,
            select_input,
            select_output,
            set_monitor,
            set_monitor_volume,
            set_routing,
            get_ui_prefs,
            set_language,
            set_view,
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
