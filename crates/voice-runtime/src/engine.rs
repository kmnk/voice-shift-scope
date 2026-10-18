//! 音声のスレッドで動く部品。どれもメモリを確保せず、ロックで待たない。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use ringbuf::traits::{Consumer, Observer, Producer};
use ringbuf::{HeapCons, HeapProd};
use voice_domain::ports::VoiceShifter;
use voice_domain::{FormantShift, PitchShift, ShiftSettings};

/// 中核の標本化の速さ。機器の速さが違うときは、ring buffer の前後で変換する。
pub const CORE_RATE: u32 = 48_000;
/// 声を変える処理に渡すブロックの長さ（10 ms）。
pub const BLOCK: usize = 480;

/// 音声のスレッドと他のスレッドで共有する値。
#[derive(Default)]
pub struct Shared {
    shift_pitch: AtomicU32,
    shift_formant: AtomicU32,
    shift_generation: AtomicU64,
    /// 分析のスレッドが書く、入力の F0（Hz、f32 の bits）。0 は無声。
    pub pitch_hint: AtomicU32,
    pub monitor_volume: AtomicU32,
    pub monitor_enabled: AtomicBool,
    /// 処理した入力の標本の数。
    pub processed: AtomicU64,
    pub output_underruns: AtomicU64,
    /// 出力が足りなくなった回数と、最初と最後のときに処理していた入力の標本の数。
    pub output_underrun_events: AtomicU64,
    pub first_underrun_at: AtomicU64,
    pub last_underrun_at: AtomicU64,
    pub monitor_underruns: AtomicU64,
    pub dropped_input: AtomicU64,
    pub stop: AtomicBool,
}

impl Shared {
    pub fn new(shift: ShiftSettings, monitor_volume: f32, monitor_enabled: bool) -> Arc<Self> {
        let s = Arc::new(Self::default());
        s.set_shift(shift);
        s.monitor_volume.store(monitor_volume.to_bits(), Ordering::Relaxed);
        s.monitor_enabled.store(monitor_enabled, Ordering::Relaxed);
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

/// 入力の 1 ブロックごとに声を変え、出力・モニター・分析の ring buffer に入れる。
/// cpal の入力のコールバックと、試験の wav の音源の両方から呼ぶ。
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
        }
    }

    /// `CORE_RATE` の mono の標本を受け取る。
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
            let block = &self.pending[start..start + BLOCK];
            self.shifter.process(block, &mut self.out_block);
            self.output.push_slice(&self.out_block);
            if self.shared.monitor_enabled.load(Ordering::Relaxed) {
                self.monitor.push_slice(&self.out_block);
            }
            self.before.push_slice(block);
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

/// 速さを変えながら ring buffer から読む。`base_ratio` は 1 出力標本あたりに進む入力標本の数
/// （中核の速さ / 機器の速さ）。溜まった量が `target` から外れた分を、±0.1% までの比で戻す。
pub struct AdaptiveReader {
    source: HeapCons<f32>,
    base_ratio: f64,
    target: f64,
    smoothed_fill: f64,
    t: f64,
    a: f32,
    b: f32,
    primed: bool,
}

impl AdaptiveReader {
    pub const MAX_CORRECTION: f64 = 0.001;

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
        }
    }

    /// 機器を替えたときに、機器の速さを替えて、溜まるまで待ち直す。
    pub fn retarget(&mut self, core_rate: u32, device_rate: u32) {
        self.base_ratio = core_rate as f64 / device_rate as f64;
        self.primed = false;
        self.smoothed_fill = self.target;
    }

    pub fn fill(&self) -> usize {
        self.source.occupied_len()
    }

    /// `out` を埋める。足りないときは 0 で埋めて、足りなかった標本の数を返す。
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let fill = self.source.occupied_len() as f64;
        if !self.primed {
            if fill < self.target {
                out.fill(0.0);
                return 0;
            }
            self.primed = true;
        }
        self.smoothed_fill += (fill - self.smoothed_fill) * 0.01;
        let correction =
            ((self.smoothed_fill - self.target) / self.target * 0.01).clamp(-Self::MAX_CORRECTION, Self::MAX_CORRECTION);
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
            // 尽きたら、また溜まるまで待つ
            self.primed = false;
        }
        missing
    }
}

/// 機器の速さから中核の速さへの、線形補間の変換（入力の側）。比は固定。
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

    /// 入力の標本を受け、変換した標本の列を返す。確保した容量を超えた分は捨てる。
    pub fn process(&mut self, input: &[f32]) -> &[f32] {
        self.out.clear();
        // t は「直前の標本 prev から input[0] へ」の位置（0〜1）で始まる
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
        // 生産の側が 0.05% 速い（機器の時計のずれ）とき、溜まった量が広がり続けない
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
        // 補正が無ければ、終わりには 960 + 12000 × 480 × 0.0005 ≈ 3840 まで溜まる
        assert!(max < 1500, "fill grew to {max}");
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
