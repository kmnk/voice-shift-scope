//! The bundled Signalsmith Stretch builds, and can process a block with pitch and formant set separately.

use voice_domain::ports::VoiceShifter;
use voice_domain::{FormantShift, PitchShift, ShiftSettings};
use voice_dsp::SignalsmithShifter;

#[test]
fn signalsmith_smoke() {
    let sample_rate = 48_000;
    let mut shifter = SignalsmithShifter::new(sample_rate);
    shifter.set(ShiftSettings { pitch: PitchShift::new(5.0).unwrap(), formant: FormantShift::new(1.1).unwrap() });
    let block = 480;
    let input: Vec<f32> = (0..block)
        .map(|i| (2.0 * std::f32::consts::PI * 150.0 * i as f32 / sample_rate as f32).sin())
        .collect();
    let mut output = vec![0.0f32; block];
    for _ in 0..40 {
        shifter.process(&input, &mut output);
    }
    assert!(shifter.latency() > 0);
    assert!(output.iter().all(|s| s.is_finite()));
    assert!(output.iter().any(|s| s.abs() > 1e-4));
}
