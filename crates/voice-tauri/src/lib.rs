//! 入る側の adapter と組み立て（composition root）。Tauri に依るのはこの crate だけ。
//! command は、画面から来た値を value object に直して use case を 1 つ呼ぶだけにする。

mod dto;
mod preferences;
mod publisher;
mod shot;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use voice_app::{AppError, VoiceStudio};
use voice_domain::{
    AnalysisSettings, DomainError, FormantCeiling, FormantShift, HarmonicRelativeLimit, HarmonicThreshold, LpcOrder, MonitorVolume,
    PitchShift, ShiftSettings,
};
use voice_dsp::DefaultAnalyzer;
use voice_runtime::{CpalDevices, RuntimePipeline};

use crate::dto::{DevicesDto, PairDto, devices_dto};
use crate::preferences::{PreferencesStore, RememberedAnalysis, RememberedDevices, RememberedShift};
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
            name: "Test source (--input-wav)".into(),
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
    /// ピッチとフォルマントを最後に書いた時刻（`change_shift`）。
    shift_saved_at: Mutex<Option<Instant>>,
}

/// An error returned to the screen.
#[derive(Debug, Serialize)]
pub struct CommandError {
    kind: &'static str,
    /// The name the screen uses to show the error in its own language (`err.<code>`). Without it, the screen shows `message` as is
    code: Option<String>,
    /// Values the screen fills into its text (`{min}`, `{max}`, `{value}`, `{device}`, `{msg}`)
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    params: BTreeMap<&'static str, String>,
    message: String,
}

/// The name of a domain error (`InvalidPitchShift` for `InvalidPitchShift { .. }`).
fn error_code(e: &DomainError) -> String {
    let s = format!("{e:?}");
    s.split(|c: char| !c.is_alphanumeric()).next().unwrap_or_default().to_string()
}

/// The values in a domain error, for the screen to fill into its text.
fn error_params(e: &DomainError) -> BTreeMap<&'static str, String> {
    let mut p = BTreeMap::new();
    match e {
        DomainError::InvalidPitchShift { value, min, max }
        | DomainError::InvalidFormantShift { value, min, max }
        | DomainError::InvalidFormantCeiling { value, min, max }
        | DomainError::InvalidHarmonicThreshold { value, min, max }
        | DomainError::InvalidHarmonicRelativeLimit { value, min, max } => {
            p.insert("value", value.to_string());
            p.insert("min", min.to_string());
            p.insert("max", max.to_string());
        }
        DomainError::InvalidLpcOrder { value, min, max } => {
            p.insert("value", value.to_string());
            p.insert("min", min.to_string());
            p.insert("max", max.to_string());
        }
        DomainError::InvalidMonitorVolume { value } | DomainError::InvalidFrequency { value } => {
            p.insert("value", value.to_string());
        }
        DomainError::UnknownDevice(device) => {
            p.insert("device", device.clone());
        }
        _ => {}
    }
    p
}

impl From<AppError> for CommandError {
    fn from(e: AppError) -> Self {
        let message = e.to_string();
        match e {
            AppError::Domain(d) => Self { kind: "rule", code: Some(error_code(&d)), params: error_params(&d), message },
            AppError::Devices(inner) => {
                Self { kind: "devices", code: Some("Devices".into()), params: BTreeMap::from([("msg", inner.to_string())]), message }
            }
            AppError::Pipeline(inner) => {
                Self { kind: "audio", code: Some("Pipeline".into()), params: BTreeMap::from([("msg", inner.to_string())]), message }
            }
        }
    }
}

impl From<DomainError> for CommandError {
    fn from(e: DomainError) -> Self {
        Self { kind: "invalid", code: Some(error_code(&e)), params: error_params(&e), message: e.to_string() }
    }
}

fn io_error(e: impl std::fmt::Display) -> CommandError {
    CommandError { kind: "io", code: None, params: BTreeMap::new(), message: e.to_string() }
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
        other => Err(CommandError {
            kind: "invalid",
            code: None,
            params: BTreeMap::new(),
            message: format!("the source must be shifted or original ({other})"),
        }),
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
const COMPACT_SIZE: (f64, f64) = (266.0, 640.0);

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
    let Some(window) = app.get_webview_window("main") else { return };
    let compact = view == "compact";
    let (w, h) = if compact {
        (COMPACT_SIZE.0, content_height.map(|h| h.clamp(200.0, 900.0)).unwrap_or(COMPACT_SIZE.1))
    } else {
        FULL_SIZE
    };
    let (min_w, min_h) = if compact { (260.0, 200.0) } else { (960.0, 640.0) };
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(min_w, min_h)));
    let _ = window.unmaximize();
    let _ = window.set_size(tauri::LogicalSize::new(w, h));
    // 窓の中の高さが求めた高さにならなかったとき（窓の縁を含めて決まる場合など）は、その差だけ直す
    if compact {
        if let (Ok(inner), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
            let actual = inner.height as f64 / scale;
            let diff = h - actual;
            if diff.abs() > 1.0 {
                let _ = window.set_size(tauri::LogicalSize::new(w, h + diff));
            }
        }
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

/// ピッチとフォルマントを替え、覚える。スライダーを動かす間は呼ばれ続けるので、書くのは SHIFT_SAVE_INTERVAL に 1 回までにし、
/// 最後の値はアプリを閉じるときに書く（`remember_current_shift`）。
#[tauri::command]
fn change_shift(state: State<AppState>, semitones: f32, formant: f32) -> Res<()> {
    let shift = ShiftSettings { pitch: PitchShift::new(semitones)?, formant: FormantShift::new(formant)? };
    state.studio.lock().expect("studio").change_shift(shift)?;
    let mut saved_at = state.shift_saved_at.lock().expect("shift_saved_at");
    if saved_at.is_none_or(|t| t.elapsed() >= SHIFT_SAVE_INTERVAL) {
        let _ = state.preferences.remember_shift(RememberedShift { semitones, formant });
        *saved_at = Some(Instant::now());
    }
    Ok(())
}

/// ピッチとフォルマントを書く間隔の下限。
const SHIFT_SAVE_INTERVAL: Duration = Duration::from_millis(500);

/// 今のピッチとフォルマントを覚える（アプリを閉じるとき）。
fn remember_current_shift(state: &AppState) {
    if let Ok(studio) = state.studio.lock() {
        let shift = studio.session().shift();
        let _ = state.preferences.remember_shift(RememberedShift { semitones: shift.pitch.semitones(), formant: shift.formant.ratio() });
    }
}

/// 覚えたピッチとフォルマントを、範囲の中なら設定にする。
fn restored_shift(remembered: Option<RememberedShift>) -> Option<ShiftSettings> {
    let r = remembered?;
    Some(ShiftSettings { pitch: PitchShift::new(r.semitones).ok()?, formant: FormantShift::new(r.formant).ok()? })
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
        return Err(io_error("the file name is not valid"));
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

/// 画面の通しの試験で、起動したときに出ているはずのピッチとフォルマント。環境変数
/// `VOICE_SHIFT_SCOPE_TEST_EXPECT_SHIFT`（"半音,倍率"）があればその値、無ければ既定（0 半音・1.0 倍）。
/// 試験の入口で起動したときだけ値を返す。
#[tauri::command]
fn selftest_expected_shift(state: State<AppState>) -> Option<(f32, f32)> {
    state.selftest_dir.as_ref()?;
    let parsed = std::env::var("VOICE_SHIFT_SCOPE_TEST_EXPECT_SHIFT").ok().and_then(|v| {
        let (a, b) = v.split_once(',')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    });
    Some(parsed.unwrap_or((0.0, 1.0)))
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

/// 試験で窓を変えてよい大きさ（論理の px）。全体の表示の最小（960×640）から、ふつうの画面に収まる大きさまで。
const TEST_SIZE_MIN: (f64, f64) = (960.0, 640.0);
const TEST_SIZE_MAX: (f64, f64) = (2560.0, 1600.0);

/// 試験で窓の大きさを変えるときの値を確かめる。範囲の外は理由を返す。
fn checked_test_size(width: f64, height: f64) -> Result<(f64, f64), String> {
    let ok = |v: f64, min: f64, max: f64| v.is_finite() && v >= min && v <= max;
    if ok(width, TEST_SIZE_MIN.0, TEST_SIZE_MAX.0) && ok(height, TEST_SIZE_MIN.1, TEST_SIZE_MAX.1) {
        Ok((width, height))
    } else {
        Err(format!(
            "窓の大きさは {}×{} から {}×{} の間（受け取ったのは {width}×{height}）",
            TEST_SIZE_MIN.0, TEST_SIZE_MIN.1, TEST_SIZE_MAX.0, TEST_SIZE_MAX.1
        ))
    }
}

/// 画面の通しの試験で、窓の中の大きさを変える（最小の窓で配置を確かめるため）。
/// 試験の入口（`--ui-selftest`）で起動したときだけ動き、利用者の画面からは使えない。
#[tauri::command]
fn selftest_resize(app: tauri::AppHandle, state: State<AppState>, width: f64, height: f64) -> Res<()> {
    if state.selftest_dir.is_none() {
        return Err(io_error("試験の入口で起動していません"));
    }
    let (w, h) = checked_test_size(width, height).map_err(io_error)?;
    let Some(window) = app.get_webview_window("main") else { return Err(io_error("窓が無い")) };
    let _ = window.unmaximize();
    window.set_size(tauri::LogicalSize::new(w, h)).map_err(io_error)?;
    selftest_log(&format!("selftest_resize {w}x{h}"));
    Ok(())
}

/// 画面の通しの試験で、受け入れ基準の項目を判定した時点の窓の画像を <dir>/shots/<name>.png に書く（Windows だけ）。
/// 撮れなかったときは理由を返し、<dir>/app-log.txt にも書く。
#[tauri::command]
fn selftest_shot(app: tauri::AppHandle, state: State<AppState>, name: String) -> Res<String> {
    let Some(dir) = state.selftest_dir.clone() else { return Err(io_error("試験の入口で起動していません")) };
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(io_error("画像の名前は英数字と - だけ"));
    }
    let shots = dir.join("shots");
    std::fs::create_dir_all(&shots).map_err(io_error)?;
    let result = match app.get_webview_window("main") {
        Some(window) => match shot::capture(&window, &shots.join(format!("{name}.png"))) {
            Ok(()) => "ok".to_string(),
            Err(e) => e,
        },
        None => "窓が無い".to_string(),
    };
    selftest_log(&format!("selftest_shot {name}: {result}"));
    Ok(result)
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
                Box::new(voice_dsp::app_shifter),
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
            // 前に選んだ機器、ピッチとフォルマント、分析の設定を戻す（一覧に無い機器や範囲の外の値は、既定のまま）
            let prefs = PreferencesStore::new(config_dir.clone()).load();
            if !test_source {
                studio.restore_devices(
                    prefs.devices.input.as_deref(),
                    prefs.devices.output.as_deref(),
                    prefs.devices.monitor.as_deref(),
                );
            }
            if let Some(shift) = restored_shift(prefs.shift) {
                let _ = studio.change_shift(shift);
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
                shift_saved_at: Mutex::new(None),
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
            selftest_shot,
            selftest_resize,
            selftest_expected_shift,
        ])
        .build(tauri::generate_context!())
        .expect("アプリを起動できませんでした")
        .run(|app, event| {
            // 閉じるときに、最後のピッチとフォルマントを覚える（動かす間は間を空けて書いているため）
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app.try_state::<AppState>()
            {
                remember_current_shift(&state);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::checked_test_size;
    use super::{AppError, CommandError, DomainError, RememberedShift, restored_shift};

    #[test]
    fn command_error_carries_the_name_and_values_of_a_domain_error() {
        let e = CommandError::from(DomainError::InvalidPitchShift { value: 30.0, min: -24.0, max: 24.0 });
        let json = serde_json::to_value(&e).expect("serialize");
        assert_eq!(json["code"], "InvalidPitchShift");
        assert_eq!(json["params"]["min"], "-24");
        assert_eq!(json["params"]["max"], "24");
        assert_eq!(json["params"]["value"], "30");
    }

    #[test]
    fn command_error_names_device_and_audio_errors_and_omits_empty_values() {
        let e = CommandError::from(AppError::Pipeline("boom".into()));
        let json = serde_json::to_value(&e).expect("serialize");
        assert_eq!(json["code"], "Pipeline");
        assert_eq!(json["params"]["msg"], "boom");
        let json = serde_json::to_value(CommandError::from(DomainError::NotRunning)).expect("serialize");
        assert_eq!(json["code"], "NotRunning");
        assert!(json.get("params").is_none());
    }

    #[test]
    fn restores_remembered_shift_only_within_range() {
        let ok = restored_shift(Some(RememberedShift { semitones: -12.0, formant: 0.8 })).expect("範囲内");
        assert_eq!((ok.pitch.semitones(), ok.formant.ratio()), (-12.0, 0.8));
        assert!(restored_shift(None).is_none());
        assert!(restored_shift(Some(RememberedShift { semitones: 99.0, formant: 1.0 })).is_none());
        assert!(restored_shift(Some(RememberedShift { semitones: 0.0, formant: 3.0 })).is_none());
    }

    #[test]
    fn test_size_accepts_the_minimum_and_default_windows() {
        assert_eq!(checked_test_size(960.0, 640.0), Ok((960.0, 640.0)));
        assert_eq!(checked_test_size(1280.0, 860.0), Ok((1280.0, 860.0)));
    }

    #[test]
    fn test_size_rejects_sizes_outside_the_range() {
        assert!(checked_test_size(959.0, 640.0).is_err());
        assert!(checked_test_size(960.0, 639.0).is_err());
        assert!(checked_test_size(3000.0, 860.0).is_err());
        assert!(checked_test_size(f64::NAN, 860.0).is_err());
    }
}
