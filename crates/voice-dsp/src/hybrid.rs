//! ピッチの設定で 2 つの声を変える処理を切り替える `HybridShifter`。
//!
//! ピッチを 0 半音より下げるときは TD-PSOLA（`PsolaShifter`）、0 半音以上では Signalsmith Stretch を使う。
//! 下げる向きで Signalsmith Stretch の周期性が大きく落ち、PSOLA の方が良かったことに依る
//! （work リポジトリの voice-shit-scope-voice-hokan のタスクの research.md）。
//! 2 つとも毎ブロック同じ入力で動かし続け、設定が境目をまたいだら 30 ms で出力を混ぜて移る。
//! PSOLA は Signalsmith Stretch と同じ遅れで作るので、切り替えても時刻は跳ばない。
//! PSOLA が無声と判定した区間は、Signalsmith Stretch の出力で埋める（`PsolaShifter::process_with_fallback`）。
//! 有声の音を無声と取りこぼしても、元の高さの声が混ざらないようにするため。
//! 2 つの処理は、それぞれ `LevelMatch` で音量を入力にそろえてから混ぜる。後ろで 1 つにそろえると、
//! 切り替えの直後に前の処理に合わせた利得が残り、音量が跳ぶ。2 つの出力は位相がそろわないので、
//! 混ぜる重みは強さの和が一定になる cos と sin にする。

use voice_domain::ShiftSettings;
use voice_domain::ports::VoiceShifter;

use crate::level::LevelMatch;
use crate::psola::PsolaShifter;
use crate::shifter::SignalsmithShifter;

const CROSSFADE_SECONDS: f32 = 0.03;

pub struct HybridShifter {
    signalsmith: SignalsmithShifter,
    psola: PsolaShifter,
    match_signalsmith: LevelMatch,
    match_psola: LevelMatch,
    identity: bool,
    use_psola: bool,
    /// PSOLA の出力の重み（0 = Signalsmith Stretch だけ、1 = PSOLA だけ）。
    mix: f32,
    step: f32,
    a: Vec<f32>,
    b: Vec<f32>,
}

impl HybridShifter {
    /// 作るときにメモリを確保するので、音声のスレッドの外で作る。`max_block` は 1 回の `process` の最大の長さ。
    pub fn new(signalsmith: SignalsmithShifter, sample_rate: u32, max_block: usize) -> Self {
        let latency = signalsmith.latency();
        Self {
            psola: PsolaShifter::new(sample_rate, latency),
            signalsmith,
            match_signalsmith: LevelMatch::new(latency, sample_rate),
            match_psola: LevelMatch::new(latency, sample_rate),
            identity: true,
            use_psola: false,
            mix: 0.0,
            step: 1.0 / (CROSSFADE_SECONDS * sample_rate as f32),
            a: vec![0.0; max_block],
            b: vec![0.0; max_block],
        }
    }

    /// 今 PSOLA に向かっているか（試験のため）。
    pub fn uses_psola(&self) -> bool {
        self.use_psola
    }

    fn wants_psola(settings: ShiftSettings) -> bool {
        settings.pitch.semitones() < 0.0
    }
}

impl VoiceShifter for HybridShifter {
    fn set(&mut self, settings: ShiftSettings) {
        self.signalsmith.set(settings);
        self.psola.set(settings);
        self.use_psola = Self::wants_psola(settings);
        self.identity = settings == ShiftSettings::NONE;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let mut done = 0;
        while done < input.len() {
            let n = (input.len() - done).min(self.a.len());
            let (x, y) = (&input[done..done + n], &mut output[done..done + n]);
            self.signalsmith.process(x, &mut self.a[..n]);
            self.psola.process_with_fallback(x, &self.a[..n], &mut self.b[..n]);
            self.match_signalsmith.apply(x, &mut self.a[..n], self.identity);
            self.match_psola.apply(x, &mut self.b[..n], self.identity);
            let target = if self.use_psola { 1.0 } else { 0.0 };
            for ((out, a), b) in y.iter_mut().zip(&self.a[..n]).zip(&self.b[..n]) {
                if self.mix != target {
                    self.mix = if target > self.mix { (self.mix + self.step).min(1.0) } else { (self.mix - self.step).max(0.0) };
                }
                // 強さの和が一定になる重み
                let angle = std::f32::consts::FRAC_PI_2 * self.mix;
                *out = a * angle.cos() + b * angle.sin();
            }
            done += n;
        }
    }

    fn latency(&self) -> usize {
        self.signalsmith.latency()
    }

    fn set_pitch_hint(&mut self, f0: f32) {
        self.signalsmith.set_pitch_hint(f0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voice_domain::{FormantShift, PitchShift};

    fn settings(st: f32) -> ShiftSettings {
        ShiftSettings { pitch: PitchShift::new(st).unwrap(), formant: FormantShift::NONE }
    }

    #[test]
    fn chooses_psola_only_below_zero_semitones() {
        let mut h = HybridShifter::new(SignalsmithShifter::new(48_000), 48_000, 480);
        for (st, psola) in [(-12.0, true), (-0.5, true), (0.0, false), (5.0, false)] {
            h.set(settings(st));
            assert_eq!(h.uses_psola(), psola, "{st}");
        }
    }

    #[test]
    fn keeps_the_signalsmith_latency() {
        let h = HybridShifter::new(SignalsmithShifter::new(48_000), 48_000, 480);
        assert_eq!(h.latency(), 3360);
    }

    #[test]
    fn matches_signalsmith_alone_at_zero_or_above() {
        let x: Vec<f32> =
            (0..48_000).map(|i| (2.0 * std::f32::consts::PI * 150.0 * i as f32 / 48_000.0).sin() * 0.3).collect();
        let mut h = HybridShifter::new(SignalsmithShifter::new(48_000), 48_000, 480);
        let mut s = crate::level::LevelMatched::new(SignalsmithShifter::new(48_000), 48_000);
        h.set(settings(5.0));
        s.set(settings(5.0));
        let (mut yh, mut ys) = (vec![0.0; 480], vec![0.0; 480]);
        for c in x.chunks(480) {
            h.process(c, &mut yh);
            s.process(c, &mut ys);
            assert_eq!(yh, ys);
        }
    }
}
