//! 受け入れの基準を、答えの分かる試験用の信号で判定する。

use serde_json::{Value, json};
use voice_domain::ports::VoiceShifter;
use voice_domain::{AnalysisSettings, FormantShift, PitchShift, ShiftSettings};
use voice_dsp::{DEFAULT_BLOCK_SECONDS, SignalsmithShifter};

/// 声を変える処理の窓の長さ（秒）。`check --block-ms` で替える。
pub static BLOCK_SECONDS: std::sync::OnceLock<f32> = std::sync::OnceLock::new();

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

/// 試験用の母音（`synth vowel --seconds 1.5`）を、Praat（Parselmouth 0.4.7、Burg 法、5 個・5500 Hz）で分析した
/// F1〜F4 の中央値（0.2 秒より後）。作り直すときは `scripts/praat_reference.sh`。
/// 2026-10-18 に macOS で出した値。合成の母音や分析の既定を替えたら作り直す。
pub const PRAAT_REFERENCE: [(f64, [f32; 4]); 3] = [
    (100.0, [720.0, 1215.9, 2576.4, 3471.5]),
    (150.0, [761.3, 1214.4, 2551.9, 3425.4]),
    (220.0, [710.1, 1231.8, 2606.6, 3525.2]),
];

/// B1: 試験用の母音（F0 100・150・220 Hz）で、推定した F1〜F4 が、同じ設定の Praat の値から ±2% 以内。
/// 正しい値（作った共鳴）との差も、参考として出す（F1 が倍音に引かれる LPC の性質で、Praat も同じだけずれる）。
/// `resonances` は信号を作る共鳴。
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

/// B3: F0 150 Hz で H1〜H8 だけを持ち、雑音より 20 dB 以上高い信号で、観測できた倍音の数が 8、
/// H1〜H8 の周波数が ±1% 以内。
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
        case: "F0 150 Hz、H1〜H8".into(),
        pass,
        detail: json!({ "observed_harmonics": count, "expected_observed": 8, "harmonics": freqs }),
    }]
}

/// 試験用の信号を、音声のスレッドと同じ 10 ms のブロックで声を変える処理に通す。
/// `input_f0` は入力の F0（Hz）で、フォルマントの包絡の目安に使う。
/// 返すのは、遅れの分を前から除いた出力と、その遅れ（標本）。
pub fn shift_signal(x: &[f32], settings: ShiftSettings, input_f0: Option<f32>) -> (Vec<f32>, usize) {
    let block = *BLOCK_SECONDS.get().unwrap_or(&DEFAULT_BLOCK_SECONDS);
    let mut shifter = SignalsmithShifter::with_block(RATE, block, block / 4.0);
    if let Some(f0) = input_f0 {
        shifter.set_pitch_hint(f0);
    }
    shifter.set(settings);
    let latency = shifter.latency();
    let block = 480;
    let mut padded = x.to_vec();
    padded.resize(x.len() + latency + block, 0.0);
    let mut out = vec![0.0f32; padded.len()];
    for (i, o) in padded.chunks(block).zip(out.chunks_mut(block)) {
        shifter.process(i, o);
    }
    out.drain(..latency);
    out.truncate(x.len());
    (out, latency)
}

fn shift(pitch: f32, formant: f32) -> ShiftSettings {
    ShiftSettings { pitch: PitchShift::new(pitch).expect("範囲内"), formant: FormantShift::new(formant).expect("範囲内") }
}

fn f0_median(frames: &[(f64, voice_domain::AnalysisFrame)], skip: f64) -> Option<f32> {
    median(frames.iter().filter(|(t, _)| *t >= skip).filter_map(|(_, f)| f.f0.map(|v| v.hz())).collect())
}

/// A1 と A2: フォルマント 1.0 倍のまま、ピッチを −12・−5・+5・+12 半音にしたときの F0 と F1〜F3。
pub fn a1_a2(settings: &AnalysisSettings) -> Vec<Outcome> {
    let mut out = Vec::new();
    for f0 in [100.0, 150.0] {
        let x = synth::vowel(f0, &SYNTH_VOWEL, RATE as f64, 2.0, 0.5);
        let before = analyze_signal(&x, RATE, settings);
        let before_f0 = f0_median(&before, SKIP);
        let before_fm = formant_medians(&before, SKIP);
        for st in [-12.0f32, -5.0, 5.0, 12.0] {
            let (y, _) = shift_signal(&x, shift(st, 1.0), before_f0);
            let after = analyze_signal(&y, RATE, settings);
            let after_f0 = f0_median(&after, 0.5);
            let expected = before_f0.map(|b| b * 2f32.powf(st / 12.0));
            let f0_diff = after_f0.zip(expected).map(|(a, e)| (a / e - 1.0) * 100.0);
            let a1 = f0_diff.is_some_and(|d| d.abs() <= 1.0);
            out.push(Outcome {
                criterion: "A1".into(),
                case: format!("F0 {f0} Hz、{st:+} 半音"),
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
            out.push(Outcome { criterion: "A2".into(), case: format!("F0 {f0} Hz、{st:+} 半音"), pass: a2, detail: json!(rows) });
        }
    }
    out
}

/// A3: ピッチ 0 半音のまま、フォルマントを 0.8・0.9・1.1・1.2 倍にしたときの F1〜F3 と F0。
pub fn a3(settings: &AnalysisSettings) -> Vec<Outcome> {
    let mut out = Vec::new();
    for f0 in [100.0, 150.0] {
        let x = synth::vowel(f0, &SYNTH_VOWEL, RATE as f64, 2.0, 0.5);
        let before = analyze_signal(&x, RATE, settings);
        let before_f0 = f0_median(&before, SKIP);
        let before_fm = formant_medians(&before, SKIP);
        for ratio in [0.8f32, 0.9, 1.1, 1.2] {
            let (y, _) = shift_signal(&x, shift(0.0, ratio), before_f0);
            // アプリと同じく、変えた後はフォルマントの倍率だけ上限も変えて分析する
            let after = analyze_signal(&y, RATE, &settings.for_shifted(FormantShift::new(ratio).expect("範囲内")));
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
                case: format!("F0 {f0} Hz、フォルマント {ratio} 倍"),
                pass,
                detail: json!({ "f0_diff_percent": f0_diff, "formants": rows }),
            });
        }
    }
    out
}

/// A4: 0 半音・1.0 倍のとき、出る音と入れた音の差が、入れた音より 30 dB 以上小さい。
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
        case: "0 半音・1.0 倍".into(),
        pass: null_db <= -30.0,
        detail: json!({ "difference_db": null_db, "required_db": -30.0, "latency_samples": latency }),
    }]
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
            other => return Err(format!("{other} はまだ判定できません（今あるのは A1〜A4・B1・B3）")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F1 を 20% ずらして作った信号では、Praat の値（ずらす前の信号のもの）から外れて、B1 が FAIL になる。
    #[test]
    fn b1_fails_when_signal_f1_is_off_by_twenty_percent() {
        let mut shifted = SYNTH_VOWEL;
        shifted[0].frequency *= 1.2;
        let outcomes = b1(&shifted, &AnalysisSettings::default());
        assert!(outcomes.iter().all(|o| !o.pass), "{outcomes:?}");
    }
}
