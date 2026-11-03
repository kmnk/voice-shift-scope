//! Runs the acceptance checks on test signals with known answers.

use serde_json::{Value, json};
use voice_domain::ports::VoiceShifter;
use voice_domain::{AnalysisSettings, FormantShift, PitchShift, ShiftSettings};
use voice_dsp::{DEFAULT_BLOCK_SECONDS, SignalsmithShifter, app_shifter_with_block};

/// Window length of the shifter (seconds). Changed with `check --block-ms`.
pub static BLOCK_SECONDS: std::sync::OnceLock<f32> = std::sync::OnceLock::new();

/// The shifter used for the checks: `app` (assembled as in the app, the default) or `signalsmith` (Signalsmith Stretch only, as before the change).
/// Changed with `check --shifter`. Used to confirm that a check catches a fault, by seeing it FAIL with the processing from before the change.
pub static SHIFTER: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Makes the shifter according to `SHIFTER` and `BLOCK_SECONDS`.
pub fn make_shifter() -> Box<dyn VoiceShifter> {
    let block = *BLOCK_SECONDS.get().unwrap_or(&DEFAULT_BLOCK_SECONDS);
    match SHIFTER.get().map(String::as_str) {
        Some("signalsmith") => Box::new(SignalsmithShifter::with_block(RATE, block, block / 4.0)),
        _ => Box::new(app_shifter_with_block(RATE, block)),
    }
}

use crate::analysis::{analyze_signal, formant_medians, median};
use crate::synth::{self, Resonance, SPEC_VOWEL, SYNTH_VOWEL};

pub const RATE: u32 = 48_000;
const SKIP: f64 = 0.2;

#[derive(Debug, Clone)]
pub struct Outcome {
    pub criterion: String,
    pub case: String,
    pub pass: bool,
    pub detail: Value,
}

/// Medians (after 0.2 s) of F1–F4 of the test vowel (`synth vowel --seconds 1.5`) analysed by Praat (Parselmouth 0.4.7, Burg method, 5 formants, 5500 Hz).
/// To regenerate, use `scripts/praat_reference.sh`.
/// Values produced on macOS on 2026-10-18. Regenerate them when the synthetic vowel or the analysis defaults change.
pub const PRAAT_REFERENCE: [(f64, [f32; 4]); 3] = [
    (100.0, [720.0, 1215.9, 2576.4, 3471.5]),
    (150.0, [761.3, 1214.4, 2551.9, 3425.4]),
    (220.0, [710.1, 1231.8, 2606.6, 3525.2]),
];

/// B1: for the test vowels (F0 100, 150, and 220 Hz), the estimated F1–F4 are within ±2% of Praat's values with the same settings.
/// The difference from the true values (the resonances used) is also shown for reference (LPC pulls F1 towards a harmonic, and Praat is off by the same amount).
/// `resonances` are the resonances that make the signal.
pub fn b1(resonances: &[Resonance], settings: &AnalysisSettings) -> Vec<Outcome> {
    PRAAT_REFERENCE
        .iter()
        .map(|&(f0, praat)| {
            let x = synth::vowel(f0, resonances, RATE as f64, 1.5, 0.5);
            let frames = analyze_signal(&x, RATE, settings);
            let med = formant_medians(&frames, SKIP);
            let mut pass = true;
            let rows: Vec<Value> = (0..4)
                .map(|i| {
                    let diff = med[i].map(|v| (v / praat[i] - 1.0) * 100.0);
                    let ok = diff.is_some_and(|d| d.abs() <= 2.0);
                    pass &= ok;
                    let truth = SPEC_VOWEL[i].frequency as f32;
                    json!({
                        "formant": format!("F{}", i + 1),
                        "praat": praat[i],
                        "estimated": med[i],
                        "diff_from_praat_percent": diff,
                        "tolerance_percent": 2.0,
                        "true_value": truth,
                        "diff_from_true_percent": med[i].map(|v| (v / truth - 1.0) * 100.0),
                        "pass": ok,
                    })
                })
                .collect();
            Outcome { criterion: "B1".into(), case: format!("F0 {f0} Hz"), pass, detail: json!(rows) }
        })
        .collect()
}

/// B3: for a signal at F0 150 Hz with only H1–H8, at least 20 dB above the noise, the number of harmonics seen is 8,
/// and the frequencies of H1–H8 are within ±1%.
pub fn b3(settings: &AnalysisSettings) -> Vec<Outcome> {
    let f0 = 150.0;
    let x = synth::harmonics(f0, 8, 40.0, RATE as f64, 1.5, 0.5);
    let frames = analyze_signal(&x, RATE, settings);
    let late: Vec<_> = frames.iter().filter(|(t, _)| *t >= SKIP).map(|(_, f)| f).collect();
    let count = median(late.iter().map(|f| f.observed_harmonics as f32).collect()).unwrap_or(0.0);
    let mut pass = count == 8.0;
    let freqs: Vec<Value> = (1..=8)
        .map(|k| {
            let m = median(
                late.iter()
                    .filter_map(|f| f.harmonics.iter().find(|h| h.number == k).map(|h| h.frequency.hz()))
                    .collect(),
            );
            let expected = f0 as f32 * k as f32;
            let diff = m.map(|v| (v / expected - 1.0) * 100.0);
            let ok = diff.is_some_and(|d| d.abs() <= 1.0);
            pass &= ok;
            json!({ "harmonic": k, "expected": expected, "estimated": m, "diff_percent": diff, "pass": ok })
        })
        .collect();
    vec![Outcome {
        criterion: "B3".into(),
        case: "F0 150 Hz, H1–H8".into(),
        pass,
        detail: json!({ "observed_harmonics": count, "expected_observed": 8, "harmonics": freqs }),
    }]
}

/// Runs a test signal through the shifter in 10 ms blocks, as on the audio thread.
/// `input_f0` is the input F0 (Hz), used as the formant envelope hint.
/// Returns the output with the delay removed from the front, and the delay (samples).
pub fn shift_signal(x: &[f32], settings: ShiftSettings, input_f0: Option<f32>) -> (Vec<f32>, usize) {
    shift_signal_switching(x, &[(0, settings)], input_f0)
}

/// `shift_signal` with settings changed along the way. `schedule` lists (input sample position, settings) in position order.
pub fn shift_signal_switching(
    x: &[f32],
    schedule: &[(usize, ShiftSettings)],
    input_f0: Option<f32>,
) -> (Vec<f32>, usize) {
    let mut shifter = make_shifter();
    if let Some(f0) = input_f0 {
        shifter.set_pitch_hint(f0);
    }
    let latency = shifter.latency();
    let block = 480;
    let mut padded = x.to_vec();
    padded.resize(x.len() + latency + block, 0.0);
    let mut out = vec![0.0f32; padded.len()];
    let mut next = 0;
    for (k, (i, o)) in padded.chunks(block).zip(out.chunks_mut(block)).enumerate() {
        while next < schedule.len() && schedule[next].0 <= k * block {
            shifter.set(schedule[next].1);
            next += 1;
        }
        shifter.process(i, o);
    }
    out.drain(..latency);
    out.truncate(x.len());
    (out, latency)
}

fn shift(pitch: f32, formant: f32) -> ShiftSettings {
    ShiftSettings { pitch: PitchShift::new(pitch).expect("within range"), formant: FormantShift::new(formant).expect("within range") }
}

fn f0_median(frames: &[(f64, voice_domain::AnalysisFrame)], skip: f64) -> Option<f32> {
    median(frames.iter().filter(|(t, _)| *t >= skip).filter_map(|(_, f)| f.f0.map(|v| v.hz())).collect())
}

/// The synthetic vowel at `f0` (2 s), with its median F0 and F1–F4 after the first `SKIP` seconds.
fn analysed_vowel(f0: f64, settings: &AnalysisSettings) -> (Vec<f32>, Option<f32>, [Option<f32>; 4]) {
    let x = synth::vowel(f0, &SYNTH_VOWEL, RATE as f64, 2.0, 0.5);
    let before = analyze_signal(&x, RATE, settings);
    let (f0, formants) = (f0_median(&before, SKIP), formant_medians(&before, SKIP));
    (x, f0, formants)
}

/// A1 and A2: F0 and F1–F3 at −12, −5, +5, and +12 semitones, with formant ×1.0.
pub fn a1_a2(settings: &AnalysisSettings) -> Vec<Outcome> {
    let mut out = Vec::new();
    for f0 in [100.0, 150.0] {
        let (x, before_f0, before_fm) = analysed_vowel(f0, settings);
        for st in [-12.0f32, -5.0, 5.0, 12.0] {
            let (y, _) = shift_signal(&x, shift(st, 1.0), before_f0);
            let after = analyze_signal(&y, RATE, settings);
            let after_f0 = f0_median(&after, 0.5);
            let expected = before_f0.map(|b| b * 2f32.powf(st / 12.0));
            let f0_diff = after_f0.zip(expected).map(|(a, e)| (a / e - 1.0) * 100.0);
            let a1 = f0_diff.is_some_and(|d| d.abs() <= 1.0);
            out.push(Outcome {
                criterion: "A1".into(),
                case: format!("F0 {f0} Hz, {st:+} st"),
                pass: a1,
                detail: json!({ "before_f0": before_f0, "after_f0": after_f0, "expected_f0": expected, "diff_percent": f0_diff, "tolerance_percent": 1.0 }),
            });
            let after_fm = formant_medians(&after, 0.5);
            let mut a2 = true;
            let rows: Vec<Value> = (0..3)
                .map(|i| {
                    let d = after_fm[i].zip(before_fm[i]).map(|(a, b)| (a / b - 1.0) * 100.0);
                    let ok = d.is_some_and(|d| d.abs() <= 5.0);
                    a2 &= ok;
                    json!({ "formant": format!("F{}", i + 1), "before": before_fm[i], "after": after_fm[i], "diff_percent": d, "pass": ok })
                })
                .collect();
            out.push(Outcome { criterion: "A2".into(), case: format!("F0 {f0} Hz, {st:+} st"), pass: a2, detail: json!(rows) });
        }
    }
    out
}

/// A3: F1–F3 and F0 at formant ×0.8, ×0.9, ×1.1, and ×1.2, with the pitch at 0 semitones.
pub fn a3(settings: &AnalysisSettings) -> Vec<Outcome> {
    let mut out = Vec::new();
    for f0 in [100.0, 150.0] {
        let (x, before_f0, before_fm) = analysed_vowel(f0, settings);
        for ratio in [0.8f32, 0.9, 1.1, 1.2] {
            let (y, _) = shift_signal(&x, shift(0.0, ratio), before_f0);
            // As in the app, the shifted audio is analysed with the ceiling changed by the formant factor
            let after = analyze_signal(&y, RATE, &settings.for_shifted(FormantShift::new(ratio).expect("within range")));
            let after_f0 = f0_median(&after, 0.5);
            let f0_diff = after_f0.zip(before_f0).map(|(a, b)| (a / b - 1.0) * 100.0);
            let mut pass = f0_diff.is_some_and(|d| d.abs() <= 1.0);
            let after_fm = formant_medians(&after, 0.5);
            let rows: Vec<Value> = (0..3)
                .map(|i| {
                    let d = after_fm[i].zip(before_fm[i]).map(|(a, b)| (a / (b * ratio) - 1.0) * 100.0);
                    let ok = d.is_some_and(|d| d.abs() <= 7.0);
                    pass &= ok;
                    json!({ "formant": format!("F{}", i + 1), "before": before_fm[i], "after": after_fm[i], "expected": before_fm[i].map(|b| b * ratio), "diff_percent": d, "pass": ok })
                })
                .collect();
            out.push(Outcome {
                criterion: "A3".into(),
                case: format!("F0 {f0} Hz, formant ×{ratio}"),
                pass,
                detail: json!({ "f0_diff_percent": f0_diff, "formants": rows }),
            });
        }
    }
    out
}

/// A4: at 0 semitones and ×1.0, the difference between the output and the input is at least 30 dB below the input.
pub fn a4() -> Vec<Outcome> {
    let x = synth::vowel(150.0, &SYNTH_VOWEL, RATE as f64, 2.0, 0.5);
    let (y, latency) = shift_signal(&x, ShiftSettings::NONE, Some(150.0));
    let start = (0.5 * RATE as f64) as usize;
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for i in start..x.len() {
        sig += (x[i] as f64).powi(2);
        err += ((y[i] - x[i]) as f64).powi(2);
    }
    let null_db = 10.0 * (err / sig).log10();
    vec![Outcome {
        criterion: "A4".into(),
        case: "0 st, ×1.0".into(),
        pass: null_db <= -30.0,
        detail: json!({ "difference_db": null_db, "required_db": -30.0, "latency_samples": latency }),
    }]
}

fn rms_db(x: &[f32]) -> f64 {
    let p = x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64;
    10.0 * p.max(1e-30).log10()
}

/// Q2: F1–F3 and F0 when lowering the pitch (F0 100 and 150 Hz).
/// −5 and −12 semitones (formant ×1.0) are within ±5% of the original; −12 semitones with formant ×0.8 and ×1.2 within ±7% of the original times that factor.
/// In every case, F0 is within ±1% of the target.
pub fn q2(settings: &AnalysisSettings) -> Vec<Outcome> {
    let mut out = Vec::new();
    for f0 in [100.0, 150.0] {
        let (x, before_f0, before_fm) = analysed_vowel(f0, settings);
        for (st, ratio, tolerance) in [(-5.0f32, 1.0f32, 5.0f32), (-12.0, 1.0, 5.0), (-12.0, 0.8, 7.0), (-12.0, 1.2, 7.0)] {
            let (y, _) = shift_signal(&x, shift(st, ratio), before_f0);
            let after = analyze_signal(&y, RATE, &settings.for_shifted(FormantShift::new(ratio).expect("within range")));
            let after_f0 = f0_median(&after, 0.5);
            let expected_f0 = before_f0.map(|b| b * 2f32.powf(st / 12.0));
            let f0_diff = after_f0.zip(expected_f0).map(|(a, e)| (a / e - 1.0) * 100.0);
            let mut pass = f0_diff.is_some_and(|d| d.abs() <= 1.0);
            let after_fm = formant_medians(&after, 0.5);
            let rows: Vec<Value> = (0..3)
                .map(|i| {
                    let d = after_fm[i].zip(before_fm[i]).map(|(a, b)| (a / (b * ratio) - 1.0) * 100.0);
                    let ok = d.is_some_and(|d| d.abs() <= tolerance);
                    pass &= ok;
                    json!({ "formant": format!("F{}", i + 1), "before": before_fm[i], "after": after_fm[i], "expected": before_fm[i].map(|b| b * ratio), "diff_percent": d, "tolerance_percent": tolerance, "pass": ok })
                })
                .collect();
            out.push(Outcome {
                criterion: "Q2".into(),
                case: format!("F0 {f0} Hz, {st:+} st, formant ×{ratio}"),
                pass,
                detail: json!({ "expected_f0": expected_f0, "after_f0": after_f0, "f0_diff_percent": f0_diff, "formants": rows }),
            });
        }
    }
    out
}

/// Q3: for a synthetic vowel at F0 150 Hz, switching from −5 to +5 semitones at 1.0 s and to −5 semitones at 2.0 s,
/// the RMS (20 ms windows, every 5 ms) in the 100 ms around each switch stays within 0.5–2 times the median of the same RMS over the 200 ms before.
/// The switch happens at processing time, which is earlier in the output by the delay, so the range covers both.
pub fn q3() -> Vec<Outcome> {
    let x = synth::vowel(150.0, &SYNTH_VOWEL, RATE as f64, 3.0, 0.5);
    let r = RATE as usize;
    let schedule = [(0, shift(-5.0, 1.0)), (r, shift(5.0, 1.0)), (2 * r, shift(-5.0, 1.0))];
    let (y, latency) = shift_signal_switching(&x, &schedule, Some(150.0));
    // 20 ms windows every 5 ms. A window longer than one output period (F0 down to 50 Hz) keeps frames between pulses from counting as dips
    let hop = r / 200;
    let width = r / 50;
    let frame_rms = |a: usize, b: usize| -> Vec<f64> {
        (a / hop..b / hop).map(|k| rms_db(&y[k * hop - width / 2..k * hop + width / 2])).collect()
    };
    [(r, "−5 → +5 st"), (2 * r, "+5 → −5 st")]
        .iter()
        .map(|&(at, name)| {
            let lo = at.saturating_sub(latency) - r / 10;
            let hi = at + r / 10;
            let reference = median(frame_rms(lo - r / 5, lo).iter().map(|v| *v as f32).collect()).unwrap_or(-300.0) as f64;
            let frames = frame_rms(lo, hi);
            let (min, max) = frames.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            let limit = 20.0 * 2f64.log10();
            let pass = min >= reference - limit && max <= reference + limit;
            Outcome {
                criterion: "Q3".into(),
                case: name.into(),
                pass,
                detail: json!({ "reference_db": reference, "min_db": min, "max_db": max, "allowed_db": limit, "range_seconds": [lo as f64 / r as f64, hi as f64 / r as f64] }),
            }
        })
        .collect()
}

/// Q4: the shifted level (RMS after 0.5 s) is within ±2 dB of the original.
pub fn q4() -> Vec<Outcome> {
    let cases = [(-12.0, 1.0), (-5.0, 1.0), (5.0, 1.0), (12.0, 1.0), (0.0, 0.8), (0.0, 1.2), (-12.0, 0.8)];
    let mut out = Vec::new();
    for f0 in [100.0, 150.0] {
        let x = synth::vowel(f0, &SYNTH_VOWEL, RATE as f64, 2.0, 0.5);
        let start = RATE as usize / 2;
        let input_db = rms_db(&x[start..]);
        for (st, ratio) in cases {
            let (y, _) = shift_signal(&x, shift(st, ratio), Some(f0 as f32));
            let output_db = rms_db(&y[start..]);
            let diff = output_db - input_db;
            out.push(Outcome {
                criterion: "Q4".into(),
                case: format!("F0 {f0} Hz, {st:+} st, formant ×{ratio}"),
                pass: diff.abs() <= 2.0,
                detail: json!({ "input_db": input_db, "output_db": output_db, "diff_db": diff, "tolerance_db": 2.0 }),
            });
        }
    }
    out
}

/// Q5: shifting white noise alone at −60 dBFS by ±12 semitones does not raise the output RMS more than 1 dB above the input.
pub fn q5() -> Vec<Outcome> {
    let n = 2 * RATE as usize;
    // Uniform noise with an RMS of −60 dBFS from a fixed-seed linear congruential generator (the RMS of a uniform distribution [−a, a] is a / √3)
    let a = 1e-3f32 * 3f32.sqrt();
    let mut state = 0x1234_5678u32;
    let x: Vec<f32> = (0..n)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * a
        })
        .collect();
    let start = RATE as usize / 2;
    let input_db = rms_db(&x[start..]);
    [12.0f32, -12.0]
        .iter()
        .map(|&st| {
            let (y, _) = shift_signal(&x, shift(st, 1.0), None);
            let output_db = rms_db(&y[start..]);
            Outcome {
                criterion: "Q5".into(),
                case: format!("−60 dBFS white noise, {st:+} st"),
                pass: output_db <= input_db + 1.0,
                detail: json!({ "input_db": input_db, "output_db": output_db, "allowed_increase_db": 1.0 }),
            }
        })
        .collect()
}

pub fn run(criteria: &[String], settings: &AnalysisSettings) -> Result<Vec<Outcome>, String> {
    let mut out = Vec::new();
    for c in criteria {
        match c.as_str() {
            "A1" | "A2" => {
                if !out.iter().any(|o: &Outcome| o.criterion == "A1") {
                    let both = a1_a2(settings);
                    out.extend(both.into_iter().filter(|o| criteria.contains(&o.criterion)));
                }
            }
            "A3" => out.extend(a3(settings)),
            "A4" => out.extend(a4()),
            "B1" => out.extend(b1(&SYNTH_VOWEL, settings)),
            "B3" => out.extend(b3(settings)),
            "Q2" => out.extend(q2(settings)),
            "Q3" => out.extend(q3()),
            "Q4" => out.extend(q4()),
            "Q5" => out.extend(q5()),
            other => return Err(format!("{other} cannot be checked yet (available: A1–A4, B1, B3, Q2–Q5)")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signal made with F1 moved by 20% falls outside Praat's values (for the signal before the move), and B1 FAILs.
    #[test]
    fn b1_fails_when_signal_f1_is_off_by_twenty_percent() {
        let mut shifted = SYNTH_VOWEL;
        shifted[0].frequency *= 1.2;
        let outcomes = b1(&shifted, &AnalysisSettings::default());
        assert!(outcomes.iter().all(|o| !o.pass), "{outcomes:?}");
    }
}
