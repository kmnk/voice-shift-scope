//! A voice shifter: `VoiceShifter`, which wraps Signalsmith Stretch 1.4.0 (MIT, bundled in third_party).

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

/// Default window length. The delay is about this length.
/// Compared with the default 120 ms, 70 ms gives almost the same results for acceptance checks A1–A4 (the same 2 FAILs on the test vowels),
/// and 50 ms broke down on a voice at F0 100 Hz (compared with voice-cli check --block-ms).
pub const DEFAULT_BLOCK_SECONDS: f32 = 0.07;

/// Ratio of F0 given as the formant envelope hint. Half the input F0 gave the smallest error on the test vowels at F0 100 and 150 Hz.
pub const FORMANT_BASE_RATIO: f32 = 0.5;

pub struct SignalsmithShifter {
    raw: *mut RawStretch,
    sample_rate: u32,
    settings: ShiftSettings,
}

// Touched from only one thread (ownership is moved to the audio thread).
unsafe impl Send for SignalsmithShifter {}

impl SignalsmithShifter {
    /// Mono, `sample_rate` Hz, window `DEFAULT_BLOCK_SECONDS`.
    /// Allocates memory when created, so create it outside the audio thread.
    pub fn new(sample_rate: u32) -> Self {
        Self::with_block(sample_rate, DEFAULT_BLOCK_SECONDS, DEFAULT_BLOCK_SECONDS / 4.0)
    }

    /// Signalsmith Stretch's default window (about 120 ms, with about 120 ms of delay).
    pub fn with_preset_default(sample_rate: u32) -> Self {
        Self::from_raw(unsafe { vss_stretch_create(sample_rate as f32) }, sample_rate)
    }

    /// Sets the window and interval in seconds. A shorter window reduces the delay and lowers the quality of low voices.
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
            // compensate_pitch: keep the formants in place when the pitch moves, then apply the factor
            vss_stretch_set_formant_factor(self.raw, self.settings.formant.ratio(), 1);
        }
    }

    /// F0 hint (Hz) for finding the formant envelope. With 0, Signalsmith Stretch estimates it itself.
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

    /// Gives half the input F0 as the formant envelope hint (the ratio with the smallest error on the test vowels).
    fn set_pitch_hint(&mut self, f0: f32) {
        if f0 > 0.0 {
            self.set_formant_base_hz(f0 * FORMANT_BASE_RATIO);
        }
    }
}
