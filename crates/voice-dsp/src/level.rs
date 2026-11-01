//! 声を変えた後の音量を、変える前の音量に合わせる `LevelMatched`。
//!
//! 中の処理と同じ遅れだけ入力を遅らせ、遅らせた入力と出力の平方の平均（時定数 300 ms）の比から利得を求める。
//! 利得は ±12 dB に制限し、時定数 50 ms で滑らかに追う。入力がほぼ無音（−50 dBFS 未満）の間は利得を止め、
//! 0 半音・1.0 倍のときは利得を 1 に戻す。

use voice_domain::ShiftSettings;
use voice_domain::ports::VoiceShifter;

const POWER_TIME_SECONDS: f32 = 0.3;
const GAIN_TIME_SECONDS: f32 = 0.05;
const MAX_GAIN_DB: f32 = 12.0;
const GATE_DBFS: f32 = -50.0;

/// 出力の音量を、同じだけ遅らせた入力の音量にそろえる部品。声を変える処理を包まずに、入力と出力の組に掛ける。
pub struct LevelMatch {
    delay: Vec<f32>,
    delay_pos: usize,
    in_power: f32,
    out_power: f32,
    gain: f32,
    power_coef: f32,
    gain_coef: f32,
    gate_power: f32,
    min_gain: f32,
    max_gain: f32,
}

impl LevelMatch {
    /// `latency` は、入力から出力までの遅れ（標本）。作るときにメモリを確保するので、音声のスレッドの外で作る。
    pub fn new(latency: usize, sample_rate: u32) -> Self {
        let fs = sample_rate as f32;
        let max_gain = 10f32.powf(MAX_GAIN_DB / 20.0);
        Self {
            delay: vec![0.0; latency.max(1)],
            delay_pos: 0,
            in_power: 0.0,
            out_power: 0.0,
            gain: 1.0,
            power_coef: 1.0 - (-1.0 / (POWER_TIME_SECONDS * fs)).exp(),
            gain_coef: 1.0 - (-1.0 / (GAIN_TIME_SECONDS * fs)).exp(),
            gate_power: 10f32.powf(GATE_DBFS / 10.0),
            min_gain: 1.0 / max_gain,
            max_gain,
        }
    }

    /// `output` に利得を掛ける。`identity`（0 半音・1.0 倍）のときは利得を 1 に戻す。
    pub fn apply(&mut self, input: &[f32], output: &mut [f32], identity: bool) {
        let n = self.delay.len();
        for (x, y) in input.iter().zip(output.iter_mut()) {
            let delayed = self.delay[self.delay_pos];
            self.delay[self.delay_pos] = *x;
            self.delay_pos = (self.delay_pos + 1) % n;

            self.in_power += (delayed * delayed - self.in_power) * self.power_coef;
            self.out_power += (*y * *y - self.out_power) * self.power_coef;
            let target = if identity {
                1.0
            } else if self.in_power < self.gate_power || self.out_power <= 1e-12 {
                self.gain
            } else {
                (self.in_power / self.out_power).sqrt().clamp(self.min_gain, self.max_gain)
            };
            self.gain += (target - self.gain) * self.gain_coef;
            *y *= self.gain;
        }
    }

    /// 今掛けている利得。
    pub fn gain(&self) -> f32 {
        self.gain
    }
}

/// 声を変える処理を包み、出力の音量を入力にそろえる。
pub struct LevelMatched<S> {
    inner: S,
    settings: ShiftSettings,
    matcher: LevelMatch,
}

impl<S: VoiceShifter> LevelMatched<S> {
    /// 作るときにメモリを確保するので、音声のスレッドの外で作る。
    pub fn new(inner: S, sample_rate: u32) -> Self {
        let matcher = LevelMatch::new(inner.latency(), sample_rate);
        Self { inner, settings: ShiftSettings::NONE, matcher }
    }

    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// 今掛けている利得（試験のため）。
    pub fn gain(&self) -> f32 {
        self.matcher.gain()
    }
}

impl<S: VoiceShifter> VoiceShifter for LevelMatched<S> {
    fn set(&mut self, settings: ShiftSettings) {
        self.settings = settings;
        self.inner.set(settings);
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        self.inner.process(input, output);
        self.matcher.apply(input, output, self.settings == ShiftSettings::NONE);
    }

    fn latency(&self) -> usize {
        self.inner.latency()
    }

    fn set_pitch_hint(&mut self, f0: f32) {
        self.inner.set_pitch_hint(f0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voice_domain::{FormantShift, PitchShift};

    /// 入力を `delay` 標本遅らせ、`gain` 倍して出す試験用の処理。
    struct Scaled {
        gain: f32,
        delay: Vec<f32>,
        pos: usize,
    }

    impl Scaled {
        fn new(gain: f32, delay: usize) -> Self {
            Self { gain, delay: vec![0.0; delay], pos: 0 }
        }
    }

    impl VoiceShifter for Scaled {
        fn set(&mut self, _: ShiftSettings) {}
        fn process(&mut self, input: &[f32], output: &mut [f32]) {
            for (x, y) in input.iter().zip(output.iter_mut()) {
                *y = self.delay[self.pos] * self.gain;
                self.delay[self.pos] = *x;
                self.pos = (self.pos + 1) % self.delay.len();
            }
        }
        fn latency(&self) -> usize {
            self.delay.len()
        }
    }

    const FS: u32 = 48_000;

    fn tone(amplitude: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * FS as f32) as usize)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * 150.0 * i as f32 / FS as f32).sin())
            .collect()
    }

    fn run(m: &mut LevelMatched<Scaled>, x: &[f32]) -> Vec<f32> {
        let mut y = vec![0.0; x.len()];
        for (i, o) in x.chunks(480).zip(y.chunks_mut(480)) {
            m.process(i, o);
        }
        y
    }

    fn rms_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10()
    }

    fn shifted() -> ShiftSettings {
        ShiftSettings { pitch: PitchShift::new(-12.0).unwrap(), formant: FormantShift::NONE }
    }

    #[test]
    fn brings_a_quieter_output_back_to_the_input_level() {
        let mut m = LevelMatched::new(Scaled::new(0.5, 3360), FS);
        m.set(shifted());
        let x = tone(0.3, 3.0);
        let y = run(&mut m, &x);
        let tail = 2 * FS as usize..x.len();
        let diff = rms_db(&y[tail.clone()]) - rms_db(&x[tail]);
        assert!(diff.abs() < 0.5, "{diff} dB");
    }

    #[test]
    fn leaves_the_output_alone_when_nothing_is_changed() {
        let mut m = LevelMatched::new(Scaled::new(0.5, 3360), FS);
        m.set(ShiftSettings::NONE);
        let x = tone(0.3, 1.0);
        let y = run(&mut m, &x);
        for i in 3360..x.len() {
            assert!((y[i] - 0.5 * x[i - 3360]).abs() < 1e-6, "{i}");
        }
    }

    #[test]
    fn does_not_raise_near_silence() {
        let mut m = LevelMatched::new(Scaled::new(0.1, 3360), FS);
        m.set(shifted());
        let x = tone(0.001, 2.0); // −63 dBFS の RMS
        run(&mut m, &x);
        assert!((m.gain() - 1.0).abs() < 1e-6, "{}", m.gain());
    }

    #[test]
    fn limits_the_gain_to_twelve_db() {
        let mut m = LevelMatched::new(Scaled::new(0.01, 3360), FS);
        m.set(shifted());
        run(&mut m, &tone(0.3, 3.0));
        assert!((20.0 * m.gain().log10() - 12.0).abs() < 0.1, "{}", m.gain());
    }
}
