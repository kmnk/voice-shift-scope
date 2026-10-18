//! 声を変える処理の実装: Signalsmith Stretch 1.4.0（MIT、third_party に取り込んだもの）を包む `VoiceShifter`。

use voice_domain::ShiftSettings;
use voice_domain::ports::VoiceShifter;

#[repr(C)]
struct RawStretch {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn vss_stretch_create(sample_rate: f32) -> *mut RawStretch;
    fn vss_stretch_create_custom(block_samples: i32, interval_samples: i32) -> *mut RawStretch;
    fn vss_stretch_destroy(s: *mut RawStretch);
    fn vss_stretch_input_latency(s: *const RawStretch) -> i32;
    fn vss_stretch_output_latency(s: *const RawStretch) -> i32;
    fn vss_stretch_set_transpose_semitones(s: *mut RawStretch, semitones: f32, tonality_limit: f32);
    fn vss_stretch_set_formant_factor(s: *mut RawStretch, multiplier: f32, compensate_pitch: i32);
    fn vss_stretch_set_formant_base(s: *mut RawStretch, relative_frequency: f32);
    fn vss_stretch_process(s: *mut RawStretch, input: *const f32, input_len: i32, output: *mut f32, output_len: i32);
}

/// 窓の長さの既定。遅れはほぼこの長さになる。
/// 既定の 120 ms と比べ、70 ms でも受け入れの基準 A1〜A4 の結果はほぼ同じで（試験用の母音で FAIL の数は同じ 2 つ）、
/// 50 ms では F0 100 Hz の声で崩れた（voice-cli check --block-ms で比べた）。
pub const DEFAULT_BLOCK_SECONDS: f32 = 0.07;

/// フォルマントの包絡の目安に渡す F0 の比。入力の F0 の半分が、F0 100・150 Hz の試験用の母音でずれが一番小さかった。
pub const FORMANT_BASE_RATIO: f32 = 0.5;

pub struct SignalsmithShifter {
    raw: *mut RawStretch,
    sample_rate: u32,
    settings: ShiftSettings,
}

// 1 つのスレッドからしか触らない（音声のスレッドへは所有ごと渡す）。
unsafe impl Send for SignalsmithShifter {}

impl SignalsmithShifter {
    /// mono、`sample_rate` Hz、窓 `DEFAULT_BLOCK_SECONDS`。
    /// 作るときにメモリを確保するので、音声のスレッドの外で作る。
    pub fn new(sample_rate: u32) -> Self {
        Self::with_block(sample_rate, DEFAULT_BLOCK_SECONDS, DEFAULT_BLOCK_SECONDS / 4.0)
    }

    /// Signalsmith Stretch の既定の窓（約 120 ms、遅れも約 120 ms）。
    pub fn with_preset_default(sample_rate: u32) -> Self {
        Self::from_raw(unsafe { vss_stretch_create(sample_rate as f32) }, sample_rate)
    }

    /// 窓と間隔を秒で指定する。窓を短くすると遅れが減り、低い声の質が落ちる。
    pub fn with_block(sample_rate: u32, block_seconds: f32, interval_seconds: f32) -> Self {
        let block = (block_seconds * sample_rate as f32) as i32;
        let interval = (interval_seconds * sample_rate as f32) as i32;
        Self::from_raw(unsafe { vss_stretch_create_custom(block, interval) }, sample_rate)
    }

    fn from_raw(raw: *mut RawStretch, sample_rate: u32) -> Self {
        assert!(!raw.is_null());
        let mut s = Self { raw, sample_rate, settings: ShiftSettings::NONE };
        s.apply();
        s
    }

    fn apply(&mut self) {
        unsafe {
            vss_stretch_set_transpose_semitones(self.raw, self.settings.pitch.semitones(), 0.0);
            // compensate_pitch: ピッチを動かしてもフォルマントを元の位置に残し、その上で倍率を掛ける
            vss_stretch_set_formant_factor(self.raw, self.settings.formant.ratio(), 1);
        }
    }

    /// フォルマントの包絡を求めるときの F0 の目安（Hz）。0 なら Signalsmith Stretch が自分で推定する。
    pub fn set_formant_base_hz(&mut self, f0: f32) {
        unsafe { vss_stretch_set_formant_base(self.raw, f0 / self.sample_rate as f32) }
    }
}

impl Drop for SignalsmithShifter {
    fn drop(&mut self) {
        unsafe { vss_stretch_destroy(self.raw) }
    }
}

impl VoiceShifter for SignalsmithShifter {
    fn set(&mut self, settings: ShiftSettings) {
        if settings != self.settings {
            self.settings = settings;
            self.apply();
        }
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        unsafe {
            vss_stretch_process(self.raw, input.as_ptr(), input.len() as i32, output.as_mut_ptr(), output.len() as i32)
        }
    }

    fn latency(&self) -> usize {
        unsafe { (vss_stretch_input_latency(self.raw) + vss_stretch_output_latency(self.raw)) as usize }
    }

    /// 入力の F0 の半分を、フォルマントの包絡の目安として渡す（試験用の母音で一番ずれが小さかった比）。
    fn set_pitch_hint(&mut self, f0: f32) {
        if f0 > 0.0 {
            self.set_formant_base_hz(f0 * FORMANT_BASE_RATIO);
        }
    }
}
