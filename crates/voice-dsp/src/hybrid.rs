//! `HybridShifter`, which switches between two shifters depending on the pitch setting.
//!
//! Below 0 semitones it uses TD-PSOLA (`PsolaShifter`); at 0 semitones and above, Signalsmith Stretch.
//! This is because Signalsmith Stretch loses much of its periodicity when lowering, and PSOLA did better
//! (checks Q1 and Q10–Q13 in docs/criteria.md).
//! Both keep running on the same input every block, and when the setting crosses the boundary the output is crossfaded over 30 ms.
//! PSOLA is built with the same delay as Signalsmith Stretch, so switching does not jump in time.
//! Parts that PSOLA judges unvoiced are filled with the Signalsmith Stretch output (`PsolaShifter::process_with_fallback`),
//! so that a voiced sound missed as unvoiced does not let the original pitch through.
//! Each shifter's output is matched to the input level with its own `LevelMatch` before mixing. Matching once after mixing would
//! leave the gain set for the previous shifter right after a switch, and the level would jump. The two outputs are not in phase,
//! so the mixing weights are cos and sin, whose powers sum to a constant.

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
    /// Weight of the PSOLA output (0 = Signalsmith Stretch only, 1 = PSOLA only).
    mix: f32,
    step: f32,
    a: Vec<f32>,
    b: Vec<f32>,
}

impl HybridShifter {
    /// Allocates memory when created, so create it outside the audio thread. `max_block` is the largest length of one `process`.
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

    /// Whether it is heading towards PSOLA now (for tests).
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
                // Weights whose powers sum to a constant
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
