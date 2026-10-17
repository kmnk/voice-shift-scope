//! Signalsmith Stretch の Rust の wrapper が build でき、
//! ピッチとフォルマントを別々に設定してブロックを処理できることを確かめる。

use signalsmith_stretch::Stretch;

#[test]
fn signalsmith_smoke() {
    let sample_rate = 48_000;
    let mut stretch = Stretch::preset_default(1, sample_rate);
    stretch.set_transpose_factor_semitones(5.0, None);
    stretch.set_formant_factor(1.1, true);

    let block = 480;
    let input: Vec<f32> = (0..block)
        .map(|i| (2.0 * std::f32::consts::PI * 150.0 * i as f32 / sample_rate as f32).sin())
        .collect();
    let mut output = vec![0.0f32; block];
    for _ in 0..20 {
        stretch.process(&input, &mut output);
    }

    assert!(stretch.input_latency() + stretch.output_latency() > 0);
    assert!(output.iter().all(|s| s.is_finite()));
    assert!(output.iter().any(|s| s.abs() > 1e-4));
}
