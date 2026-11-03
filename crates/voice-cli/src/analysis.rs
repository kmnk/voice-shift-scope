//! Runs a wav through the analysis core.

use voice_domain::ports::Analyzer;
use voice_domain::{AnalysisFrame, AnalysisSettings, SampleRate};
use voice_dsp::{ANALYSIS_WINDOW, DefaultAnalyzer};

/// Analysis interval (10 ms at 48 kHz).
pub const HOP: usize = 480;

/// Every 10 ms, analyses a window centred on that time. Returns (time in seconds, result).
pub fn analyze_signal(x: &[f32], rate: u32, settings: &AnalysisSettings) -> Vec<(f64, AnalysisFrame)> {
    analyze_signal_with(&mut DefaultAnalyzer::default(), x, rate, settings, None)
}

/// The same as `analyze_signal`, but takes the analysis parts. With `times`, adds the processing time (ms) of each analysis.
pub fn analyze_signal_with(
    analyzer: &mut DefaultAnalyzer,
    x: &[f32],
    rate: u32,
    settings: &AnalysisSettings,
    mut times: Option<&mut Vec<f64>>,
) -> Vec<(f64, AnalysisFrame)> {
    let rate_v = SampleRate::new(rate).expect("greater than 0");
    let half = ANALYSIS_WINDOW / 2;
    let mut out = Vec::new();
    let mut center = half;
    while center + half <= x.len() {
        let started = std::time::Instant::now();
        let frame = analyzer.analyze(&x[center - half..center + half], rate_v, settings);
        if let Some(t) = times.as_deref_mut() {
            t.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        out.push((center as f64 / rate as f64, frame));
        center += HOP;
    }
    out
}

pub fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(v[v.len() / 2])
}

/// Medians of F1–F4 over the frames after `skip` seconds.
pub fn formant_medians(frames: &[(f64, AnalysisFrame)], skip: f64) -> [Option<f32>; 4] {
    let mut out = [None; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = median(
            frames
                .iter()
                .filter(|(t, _)| *t >= skip)
                .filter_map(|(_, f)| f.formants[i].map(|fm| fm.frequency.hz()))
                .collect(),
        );
    }
    out
}

/// One CSV row.
pub fn csv_header() -> String {
    let mut cols = vec!["time".to_string(), "voiced".into(), "f0".into()];
    cols.extend((1..=4).map(|i| format!("f{i}")));
    cols.extend((1..=4).map(|i| format!("b{i}")));
    cols.push("observed_harmonics".into());
    cols.extend((1..=10).map(|i| format!("h{i}")));
    cols.extend((1..=10).map(|i| format!("h{i}_db")));
    cols.join(",")
}

pub fn csv_row(time: f64, f: &AnalysisFrame) -> String {
    let opt = |v: Option<f32>| v.map(|x| format!("{x:.2}")).unwrap_or_default();
    let mut cols = vec![format!("{time:.3}"), (f.voiced as u8).to_string(), opt(f.f0.map(|v| v.hz()))];
    cols.extend(f.formants.iter().map(|fm| opt(fm.map(|x| x.frequency.hz()))));
    cols.extend(f.formants.iter().map(|fm| opt(fm.map(|x| x.bandwidth.hz()))));
    cols.push(f.observed_harmonics.to_string());
    cols.extend((0..10).map(|i| opt(f.harmonics.get(i).map(|h| h.frequency.hz()))));
    cols.extend((0..10).map(|i| opt(f.harmonics.get(i).map(|h| h.level.db()))));
    cols.join(",")
}
