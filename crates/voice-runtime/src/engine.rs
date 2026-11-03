//! Parts that run on the audio thread. None of them allocates memory or waits on a lock.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use ringbuf::traits::{Consumer, Observer, Producer};
use ringbuf::{HeapCons, HeapProd};
use voice_domain::ports::VoiceShifter;
use voice_domain::{FormantShift, PitchShift, ShiftSettings};

/// The core sample rate. When a device's rate differs, it is converted before or after the ring buffer.
pub const CORE_RATE: u32 = 48_000;
/// Length of the blocks given to the shifter (10 ms).
pub const BLOCK: usize = 480;

/// Values shared between the audio thread and other threads.
#[derive(Default)]
pub struct Shared {
    shift_pitch: AtomicU32,
    shift_formant: AtomicU32,
    shift_generation: AtomicU64,
    /// The input F0 written by the analysis thread (Hz, as f32 bits). 0 is unvoiced.
    pub pitch_hint: AtomicU32,
    pub monitor_volume: AtomicU32,
    pub monitor_enabled: AtomicBool,
    /// If true, the original audio is sent (delayed by the shifter's delay, so switching does not jump in time).
    pub output_original: AtomicBool,
    pub monitor_original: AtomicBool,
    /// While false, the analysis thread finds only the F0 hint and does no display analysis or sending.
    pub analysis_display: AtomicBool,
    /// Number of input samples processed.
    pub processed: AtomicU64,
    pub output_underruns: AtomicU64,
    /// Amount in the output ring buffer (samples) and the correction applied (ppm). For tests and the status display.
    pub output_fill: AtomicU64,
    pub output_correction_ppm: std::sync::atomic::AtomicI64,
    /// Number of output underruns, and the number of input samples processed at the first and last one.
    pub output_underrun_events: AtomicU64,
    pub first_underrun_at: AtomicU64,
    pub last_underrun_at: AtomicU64,
    pub monitor_underruns: AtomicU64,
    pub dropped_input: AtomicU64,
    /// Number of Windows buffer overruns (Xrun) reported by cpal.
    pub input_xruns: AtomicU64,
    pub last_input_xrun_at: AtomicU64,
    pub output_xruns: AtomicU64,
    pub monitor_xruns: AtomicU64,
    pub stop: AtomicBool,
}

impl Shared {
    pub fn new(shift: ShiftSettings, monitor_volume: f32, monitor_enabled: bool) -> Arc<Self> {
        let s = Arc::new(Self::default());
        s.set_shift(shift);
        s.monitor_volume.store(monitor_volume.to_bits(), Ordering::Relaxed);
        s.monitor_enabled.store(monitor_enabled, Ordering::Relaxed);
        s.analysis_display.store(true, Ordering::Relaxed);
        s
    }

    pub fn set_shift(&self, shift: ShiftSettings) {
        self.shift_pitch.store(shift.pitch.semitones().to_bits(), Ordering::Relaxed);
        self.shift_formant.store(shift.formant.ratio().to_bits(), Ordering::Relaxed);
        self.shift_generation.fetch_add(1, Ordering::Release);
    }

    pub fn current_shift(&self) -> Option<ShiftSettings> {
        Some(ShiftSettings {
            pitch: PitchShift::new(f32::from_bits(self.shift_pitch.load(Ordering::Relaxed))).ok()?,
            formant: FormantShift::new(f32::from_bits(self.shift_formant.load(Ordering::Relaxed))).ok()?,
        })
    }
}

/// Shifts the voice for each input block and puts it into the output, monitor, and analysis ring buffers.
/// Called from both the cpal input callback and the test wav source.
pub struct InputProcessor {
    shifter: Box<dyn VoiceShifter>,
    shared: Arc<Shared>,
    pending: Vec<f32>,
    out_block: Vec<f32>,
    output: HeapProd<f32>,
    monitor: HeapProd<f32>,
    before: HeapProd<f32>,
    after: HeapProd<f32>,
    seen_generation: u64,
    seen_hint: u32,
    /// Ring buffer that delays the original audio by as much as the shifter
    delay: Vec<f32>,
    delay_pos: usize,
    delayed_block: Vec<f32>,
}

impl InputProcessor {
    pub fn new(
        shifter: Box<dyn VoiceShifter>,
        shared: Arc<Shared>,
        output: HeapProd<f32>,
        monitor: HeapProd<f32>,
        before: HeapProd<f32>,
        after: HeapProd<f32>,
    ) -> Self {
        Self {
            shifter,
            shared,
            pending: Vec::with_capacity(BLOCK * 64),
            out_block: vec![0.0; BLOCK],
            output,
            monitor,
            before,
            after,
            seen_generation: u64::MAX,
            seen_hint: 0,
            delay: Vec::new(),
            delay_pos: 0,
            delayed_block: vec![0.0; BLOCK],
        }
    }

    /// Sets how long the original audio is delayed (the shifter's delay). Call before running on the audio thread.
    pub fn with_original_delay(mut self, samples: usize) -> Self {
        self.delay = vec![0.0; samples.max(1)];
        self.delay_pos = 0;
        self
    }

    fn delay_original(&mut self, start: usize) {
        if self.delay.is_empty() {
            self.delayed_block.copy_from_slice(&self.pending[start..start + BLOCK]);
            return;
        }
        let n = self.delay.len();
        for i in 0..BLOCK {
            let x = self.pending[start + i];
            self.delayed_block[i] = self.delay[self.delay_pos];
            self.delay[self.delay_pos] = x;
            self.delay_pos = (self.delay_pos + 1) % n;
        }
    }

    /// Takes mono samples at `CORE_RATE`.
    pub fn push(&mut self, samples: &[f32]) {
        let room = self.pending.capacity() - self.pending.len();
        let take = samples.len().min(room);
        if take < samples.len() {
            self.shared.dropped_input.fetch_add((samples.len() - take) as u64, Ordering::Relaxed);
        }
        self.pending.extend_from_slice(&samples[..take]);
        let mut start = 0;
        while self.pending.len() - start >= BLOCK {
            self.update_settings();
            self.shifter.process(&self.pending[start..start + BLOCK], &mut self.out_block);
            self.delay_original(start);
            if self.shared.output_original.load(Ordering::Relaxed) {
                self.output.push_slice(&self.delayed_block);
            } else {
                self.output.push_slice(&self.out_block);
            }
            if self.shared.monitor_enabled.load(Ordering::Relaxed) {
                if self.shared.monitor_original.load(Ordering::Relaxed) {
                    self.monitor.push_slice(&self.delayed_block);
                } else {
                    self.monitor.push_slice(&self.out_block);
                }
            }
            self.before.push_slice(&self.pending[start..start + BLOCK]);
            self.after.push_slice(&self.out_block);
            self.shared.processed.fetch_add(BLOCK as u64, Ordering::Relaxed);
            start += BLOCK;
        }
        self.pending.drain(..start);
    }

    fn update_settings(&mut self) {
        let generation = self.shared.shift_generation.load(Ordering::Acquire);
        if generation != self.seen_generation {
            self.seen_generation = generation;
            if let Some(shift) = self.shared.current_shift() {
                self.shifter.set(shift);
            }
        }
        let hint = self.shared.pitch_hint.load(Ordering::Relaxed);
        if hint != self.seen_hint && hint != 0 {
            self.seen_hint = hint;
            self.shifter.set_pitch_hint(f32::from_bits(hint));
        }
    }
}

/// Reads from a ring buffer while varying the rate. `base_ratio` is the number of input samples advanced per output sample
/// (core rate / device rate). The deviation of the amount buffered from `target` is corrected with a ratio of up to ±0.1%.
pub struct AdaptiveReader {
    source: HeapCons<f32>,
    base_ratio: f64,
    target: f64,
    smoothed_fill: f64,
    t: f64,
    a: f32,
    b: f32,
    primed: bool,
    last_correction: f64,
}

impl AdaptiveReader {
    /// Upper limit of the correction ratio. On CI runners the wall clock and the device clock drifted by more than ±0.1%, and ±0.1% could not keep up.
    /// 0.5% is about 9 cents in pitch, applied only while the amount buffered is off target.
    pub const MAX_CORRECTION: f64 = 0.005;
    /// When this much more than the target is buffered, drop the excess at once (20 ms at the core rate).
    pub const SNAP_EXCESS: usize = 960;

    pub fn new(source: HeapCons<f32>, core_rate: u32, device_rate: u32, target: usize) -> Self {
        Self {
            source,
            base_ratio: core_rate as f64 / device_rate as f64,
            target: target as f64,
            smoothed_fill: target as f64,
            t: 0.0,
            a: 0.0,
            b: 0.0,
            primed: false,
            last_correction: 0.0,
        }
    }

    /// The correction ratio applied last (for tests and the status display).
    pub fn correction(&self) -> f64 {
        self.last_correction
    }

    /// When the device changes, changes the device rate and waits to fill again.
    pub fn retarget(&mut self, core_rate: u32, device_rate: u32) {
        self.base_ratio = core_rate as f64 / device_rate as f64;
        self.primed = false;
        self.smoothed_fill = self.target;
    }

    pub fn fill(&self) -> usize {
        self.source.occupied_len()
    }

    /// Fills `out`. When short, fills with 0 and returns the number of missing samples.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let fill = self.source.occupied_len() as f64;
        if !self.primed {
            if fill < self.target {
                out.fill(0.0);
                return 0;
            }
            self.primed = true;
        }
        // If too much is buffered at the start or after a stall, drop the excess at once to recover the delay (±0.1% correction would take tens of seconds)
        if fill > self.target + Self::SNAP_EXCESS as f64 {
            let excess = (fill - self.target) as usize;
            self.source.skip(excess);
            self.smoothed_fill = self.target;
        }
        let fill = self.source.occupied_len() as f64;
        self.smoothed_fill += (fill - self.smoothed_fill) * 0.01;
        let correction =
            ((self.smoothed_fill - self.target) / self.target * 0.01).clamp(-Self::MAX_CORRECTION, Self::MAX_CORRECTION);
        self.last_correction = correction;
        let step = self.base_ratio * (1.0 + correction);
        let mut missing = 0;
        for o in out.iter_mut() {
            while self.t >= 1.0 {
                self.a = self.b;
                match self.source.try_pop() {
                    Some(v) => self.b = v,
                    None => {
                        missing += 1;
                        self.b = 0.0;
                    }
                }
                self.t -= 1.0;
            }
            *o = self.a + (self.b - self.a) * self.t as f32;
            self.t += step;
        }
        if missing > 0 {
            // When empty, wait to fill again
            self.primed = false;
        }
        missing
    }
}

/// Linear interpolation from the device rate to the core rate (input side). The ratio is fixed.
pub struct StreamResampler {
    step: f64,
    t: f64,
    prev: f32,
    out: Vec<f32>,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32, max_block: usize) -> Self {
        let step = from as f64 / to as f64;
        Self { step, t: 0.0, prev: 0.0, out: Vec::with_capacity(((max_block as f64 / step) as usize + 4) * 2) }
    }

    pub fn is_identity(&self) -> bool {
        self.step == 1.0
    }

    /// Takes input samples and returns the converted samples. Anything over the allocated capacity is dropped.
    pub fn process(&mut self, input: &[f32]) -> &[f32] {
        self.out.clear();
        // t starts as the position from the previous sample prev to input[0] (0 to 1)
        let mut idx = 0usize;
        loop {
            let (a, b) = if idx == 0 { (self.prev, input.first().copied().unwrap_or(self.prev)) } else { (input[idx - 1], input[idx]) };
            while self.t < 1.0 {
                if self.out.len() == self.out.capacity() {
                    break;
                }
                self.out.push(a + (b - a) * self.t as f32);
                self.t += self.step;
            }
            self.t -= 1.0;
            idx += 1;
            if idx >= input.len() {
                break;
            }
        }
        if let Some(last) = input.last() {
            self.prev = *last;
        }
        &self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::HeapRb;
    use ringbuf::traits::Split;

    #[test]
    fn adaptive_reader_keeps_fill_near_target_with_slightly_fast_producer() {
        // When the producer is 0.05% fast (device clock drift), the amount buffered does not keep growing
        let (mut prod, cons) = HeapRb::<f32>::new(48_000).split();
        let mut reader = AdaptiveReader::new(cons, 48_000, 48_000, 960);
        let mut out = vec![0.0f32; 480];
        let mut carry = 0.0f64;
        let mut fills = Vec::new();
        for _ in 0..12_000 {
            carry += 480.0 * 1.0005;
            let n = carry as usize;
            carry -= n as f64;
            prod.push_slice(&vec![0.1f32; n]);
            reader.read(&mut out);
            fills.push(reader.fill());
        }
        let late = &fills[fills.len() - 500..];
        let max = *late.iter().max().unwrap();
        // Without correction, it would reach 960 + 12000 × 480 × 0.0005 ≈ 3840 by the end
        assert!(max < 1500, "fill grew to {max}");
    }

    #[test]
    fn adaptive_reader_follows_producer_slower_by_three_tenths_of_a_percent() {
        // When the producer is 0.3% slow (a larger drift, as seen on CI runners), it does not run dry after the start
        let (mut prod, cons) = HeapRb::<f32>::new(48_000).split();
        let mut reader = AdaptiveReader::new(cons, 48_000, 48_000, 1440);
        let mut out = vec![0.0f32; 480];
        let mut carry = 0.0f64;
        let mut missing_after_warmup = 0;
        for i in 0..30_000 {
            carry += 480.0 * 0.997;
            let n = carry as usize;
            carry -= n as f64;
            prod.push_slice(&vec![0.1f32; n]);
            let missing = reader.read(&mut out);
            if i > 3000 {
                missing_after_warmup += missing;
            }
        }
        assert_eq!(missing_after_warmup, 0);
    }

    struct Silence;
    impl VoiceShifter for Silence {
        fn set(&mut self, _: ShiftSettings) {}
        fn process(&mut self, _input: &[f32], output: &mut [f32]) {
            output.fill(0.0);
        }
        fn latency(&self) -> usize {
            700
        }
    }

    /// Choosing the original audio for the output sends the input delayed by the same amount as the shifter.
    #[test]
    fn original_routing_outputs_input_delayed_by_shifter_latency() {
        let shared = Shared::new(ShiftSettings::NONE, 1.0, false);
        shared.output_original.store(true, Ordering::Relaxed);
        let (out_prod, mut out_cons) = HeapRb::<f32>::new(48_000).split();
        let (mon_prod, _m) = HeapRb::<f32>::new(48_000).split();
        let (b_prod, _b) = HeapRb::<f32>::new(48_000).split();
        let (a_prod, _a) = HeapRb::<f32>::new(48_000).split();
        let mut p = InputProcessor::new(Box::new(Silence), shared.clone(), out_prod, mon_prod, b_prod, a_prod)
            .with_original_delay(700);
        let input: Vec<f32> = (0..4800).map(|i| i as f32).collect();
        p.push(&input);
        let mut out = vec![0.0f32; 4800];
        out_cons.pop_slice(&mut out);
        assert!(out[..700].iter().all(|v| *v == 0.0));
        assert_eq!(out[700], 0.0);
        assert_eq!(out[701], 1.0);
        assert_eq!(out[4799], 4099.0);
        // Switching back to shifted gives the shifter's output (silence here)
        shared.output_original.store(false, Ordering::Relaxed);
        p.push(&vec![1.0f32; 480]);
        let mut next = vec![9.0f32; 480];
        out_cons.pop_slice(&mut next);
        assert!(next.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn stream_resampler_converts_rate() {
        let mut r = StreamResampler::new(44_100, 48_000, 4410);
        let mut total = 0;
        for _ in 0..10 {
            total += r.process(&vec![0.0; 4410]).len();
        }
        assert!((total as i64 - 48_000).abs() < 5, "{total}");
    }
}
