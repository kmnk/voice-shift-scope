//! 受け入れの基準を、答えの分かる試験用の信号で判定する。

use serde_json::{Value, json};
use voice_domain::AnalysisSettings;

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

/// B1: 合成の母音で、推定した F1〜F3 が ±5%、F4 が ±8% 以内。
/// `resonances` は信号を作る共鳴で、正しい値は常に spec の値（SPEC_VOWEL）と比べる。
pub fn b1(resonances: &[Resonance], settings: &AnalysisSettings) -> Vec<Outcome> {
    let tolerance = [5.0, 5.0, 5.0, 8.0];
    [100.0, 150.0]
        .iter()
        .map(|&f0| {
            let x = synth::vowel(f0, resonances, RATE as f64, 1.5, 0.5);
            let frames = analyze_signal(&x, RATE, settings);
            let med = formant_medians(&frames, SKIP);
            let mut pass = true;
            let rows: Vec<Value> = (0..4)
                .map(|i| {
                    let expected = SPEC_VOWEL[i].frequency as f32;
                    let diff = med[i].map(|v| (v / expected - 1.0) * 100.0);
                    let ok = diff.is_some_and(|d| d.abs() <= tolerance[i]);
                    pass &= ok;
                    json!({ "formant": format!("F{}", i + 1), "expected": expected, "estimated": med[i], "diff_percent": diff, "tolerance_percent": tolerance[i], "pass": ok })
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

pub fn run(criteria: &[String], settings: &AnalysisSettings) -> Result<Vec<Outcome>, String> {
    let mut out = Vec::new();
    for c in criteria {
        match c.as_str() {
            "B1" => out.extend(b1(&SYNTH_VOWEL, settings)),
            "B3" => out.extend(b3(settings)),
            other => return Err(format!("{other} はまだ判定できません（今あるのは B1・B3）")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F1 を 20% ずらして作った信号では、B1 が FAIL になる。
    #[test]
    fn b1_fails_when_signal_f1_is_off_by_twenty_percent() {
        let mut shifted = SYNTH_VOWEL;
        shifted[0].frequency *= 1.2;
        let outcomes = b1(&shifted, &AnalysisSettings::default());
        assert!(outcomes.iter().all(|o| !o.pass), "{outcomes:?}");
    }
}
