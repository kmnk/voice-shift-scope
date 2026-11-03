//! `PsolaShifter`, which changes the voice with TD-PSOLA.
//!
//! Places a pitch mark (reference point) at each period of the input F0, and overlaps two-period Hann-windowed grains at the new period.
//! The formants move by stretching each grain in time by `1 / factor`. While unvoiced, grains every 5 ms are overlapped at the same interval
//! (unvoiced grains are stretched too).
//!
//! In `process_with_fallback`, the parts judged unvoiced that still have some periodicity (likely missed voicing) are filled
//! with the output of another given shifter (with the same delay). Unvoiced grains do not move the pitch, so voicing missed as
//! unvoiced lets the original pitch through. Parts with almost no periodicity (breath noise such as the consonants s and sh) stay as unvoiced
//! grains (pitch unchanged, formants moved). Noise has no pitch, and lowering it with the other shifter lowers its high components and muffles it.
//!
//! The delay is set at creation (`HybridShifter` passes the same value as Signalsmith Stretch). When input up to time n has arrived,
//! the output up to time n − delay is final. The lookahead for F0 estimation, pitch mark search, and grain extraction fits within the delay.
//! Assumes an input F0 of 60–800 Hz and a formant factor of 0.5–2.0, and needs a delay of 70 ms or more.
//!
//! All buffers are allocated at creation; `process` does not allocate.

use voice_domain::ShiftSettings;
use voice_domain::ports::VoiceShifter;

const MIN_F0: f32 = 60.0;
const MAX_F0: f32 = 800.0;
const MIN_FORMANT: f32 = 0.5;
/// Interval of unvoiced grains (5 ms). Grains are twice this long.
const UNVOICED_HOP_SECONDS: f32 = 0.005;
/// Decimation of the signal used to find F0 (48 kHz → 12 kHz).
const DECIMATION: usize = 4;
/// Length of the moving average of the signal used to pick pitch marks (1 ms).
const SMOOTH_SECONDS: f32 = 0.001;
/// Thresholds of YIN's cumulative-mean-normalised difference. Strict at the start of voicing; while voicing continues, a looser threshold is used
/// within ±20% of the previous period (to avoid dropping to unvoiced on variation or breath noise, while the start guards against taking a harmonic for F0).
const YIN_THRESHOLD: f32 = 0.15;
/// Conditions for moving a pitch mark onto the glottal pulse: the distance from the mark (as a fraction of the period), and the ratio of the pulse peak to the value at the mark.
/// At distance 0.25 the difference between odd and even harmonics on Kyoko's "konnichiwa" was −1.7 dB; at 0.15 it was −1.1 dB (PSOLA prototype −0.9 dB).
const RESYNC_DISTANCE: f32 = 0.15;
const RESYNC_DOMINANCE: f32 = 1.5;
const YIN_CONTINUE_THRESHOLD: f32 = 0.4;
/// Unvoiced frames are treated as aperiodic noise where the minimum of YIN's cumulative-mean-normalised difference is above NOISE_APERIODICITY,
/// or above NOISE_APERIODICITY_HIGH_BAND with a spectral centroid (from the ratio of the power of the difference to the power of the signal)
/// above NOISE_MIN_FREQUENCY (the thresholds are the pair that satisfied both Q10 and Q13 on two text-to-speech voices). The centroid separates missed voicing (biased to low frequencies) from
/// the consonants s and sh (biased to high frequencies). Separating by periodicity alone, lowering the threshold to 0.4 left 4.9% of the frames at the original pitch for Daniel
/// at −12 semitones, and at 0.5 part of the consonants were lowered by the other shifter and their centroid fell by 12%.
const NOISE_APERIODICITY: f32 = 0.5;
const NOISE_APERIODICITY_HIGH_BAND: f32 = 0.3;
const NOISE_MIN_FREQUENCY: f32 = 5000.0;
/// RMS below which the input is unvoiced.
const SILENCE_DBFS: f32 = -50.0;

/// When voicing is found, how many seconds just before it, without pitch marks yet, are filled back as voiced at the same F0.
/// The YIN window (about 33 ms) holds only half voicing at word onsets, so voicing is detected late.
const ONSET_BACKFILL_SECONDS: f32 = 0.025;

/// Time constant that smooths the weight for mixing with the other shifter's output at voiced/unvoiced changes.
const VOICED_SMOOTH_SECONDS: f32 = 0.005;

const HISTORY: usize = 1 << 14;
const MARKS: usize = 1 << 10;
const F0_SLOTS: usize = 1 << 10;
const DECIMATED: usize = 1 << 11;

/// Second-order low-pass (RBJ).
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn lowpass(fs: f32, fc: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * fc / fs;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let cos = w.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

#[derive(Clone, Copy, Default)]
struct Mark {
    pos: i64,
    /// Period (samples). The unvoiced grain interval when unvoiced.
    period: f32,
    voiced: bool,
    /// Whether an unvoiced part is aperiodic noise.
    noise: bool,
}

pub struct PsolaShifter {
    fs: f32,
    latency: usize,
    pitch_ratio: f32,
    formant_ratio: f32,
    /// Number of input samples received (the time of the next input).
    n: i64,
    history: Vec<f32>,
    /// Centred 1 ms moving average (written `smooth_half` samples late).
    smooth: Vec<f32>,
    smooth_sum: f32,
    smooth_len: usize,
    smooth_half: usize,
    // F0 estimation
    lowpass: Biquad,
    decimated: Vec<f32>,
    decimated_count: i64,
    yin_hop: usize,
    yin_tau_min: usize,
    yin_tau_max: usize,
    yin_window: Vec<f32>,
    yin_diff: Vec<f32>,
    /// F0 of the last estimate (0 is unvoiced).
    yin_last: f32,
    /// F0 every 5 ms (0 is unvoiced). Indexed by "time / unvoiced interval".
    f0_slots: Vec<f32>,
    /// Minimum of YIN's cumulative-mean-normalised difference for each frame (larger means less periodic).
    aperiodicity_slots: Vec<f32>,
    /// Spectral centroid of each frame (Hz).
    centroid_slots: Vec<f32>,
    yin_min: f32,
    last_slot: i64,
    unvoiced_hop: f32,
    // Pitch marks
    marks: Vec<Mark>,
    marks_len: i64,
    cursor: i64,
    /// The input time of the next output to finalise. No grains are added before this.
    emitted: i64,
    onset_backfill_slots: i64,
    // Synthesis
    next_synthesis: f64,
    last_voiced: bool,
    max_half: usize,
    /// Where voiced and unvoiced grains are overlapped. Indexed by input time.
    accum: Vec<f32>,
    accum_unvoiced: Vec<f32>,
    /// Voiced (1) or unvoiced (0) at each synthesis time. Used as the weight for mixing with the other shifter in `process_with_fallback`.
    voiced_flag: Vec<f32>,
    voiced_smooth: f32,
    /// Whether each synthesis time is aperiodic unvoiced (1). Those parts are not filled by the other shifter; unvoiced grains are output.
    noise_flag: Vec<f32>,
    noise_smooth: f32,
    voiced_coef: f32,
}

// Touched from only one thread.
unsafe impl Send for PsolaShifter {}

impl PsolaShifter {
    /// `latency` is the delay from input to output (samples). Below 70 ms the lookahead is not enough.
    /// Allocates memory when created, so create it outside the audio thread.
    pub fn new(sample_rate: u32, latency: usize) -> Self {
        let fs = sample_rate as f32;
        assert!(latency as f32 >= 0.07 * fs, "PSOLA needs a delay of 70 ms or more");
        let dfs = fs / DECIMATION as f32;
        let yin_tau_max = (dfs / MIN_F0).ceil() as usize;
        let unvoiced_hop = (UNVOICED_HOP_SECONDS * fs).round();
        let smooth_len = ((SMOOTH_SECONDS * fs) as usize).max(2);
        let max_half = ((fs / MIN_F0) / MIN_FORMANT).ceil() as usize + 2;
        Self {
            fs,
            latency,
            pitch_ratio: 1.0,
            formant_ratio: 1.0,
            n: 0,
            history: vec![0.0; HISTORY],
            smooth: vec![0.0; HISTORY],
            smooth_sum: 0.0,
            smooth_len,
            smooth_half: smooth_len / 2,
            lowpass: Biquad::lowpass(fs, 2000.0),
            decimated: vec![0.0; DECIMATED],
            decimated_count: 0,
            yin_hop: (unvoiced_hop as usize) / DECIMATION,
            yin_tau_min: (dfs / MAX_F0).floor() as usize,
            yin_tau_max,
            yin_window: vec![0.0; 2 * yin_tau_max],
            yin_diff: vec![0.0; yin_tau_max + 2],
            yin_last: 0.0,
            f0_slots: vec![0.0; F0_SLOTS],
            aperiodicity_slots: vec![1.0; F0_SLOTS],
            centroid_slots: vec![0.0; F0_SLOTS],
            yin_min: 1.0,
            last_slot: -1,
            unvoiced_hop,
            marks: vec![Mark::default(); MARKS],
            marks_len: 0,
            cursor: 0,
            emitted: 0,
            onset_backfill_slots: ((ONSET_BACKFILL_SECONDS * fs) / unvoiced_hop).round() as i64,
            next_synthesis: 0.0,
            last_voiced: false,
            max_half,
            accum: vec![0.0; HISTORY],
            accum_unvoiced: vec![0.0; HISTORY],
            voiced_flag: vec![0.0; HISTORY],
            voiced_smooth: 0.0,
            noise_flag: vec![0.0; HISTORY],
            noise_smooth: 0.0,
            voiced_coef: 1.0 - (-1.0 / (VOICED_SMOOTH_SECONDS * fs)).exp(),
        }
    }

    fn input_at(&self, i: i64) -> f32 {
        if i < 0 || i >= self.n || i <= self.n - HISTORY as i64 {
            0.0
        } else {
            self.history[i as usize & (HISTORY - 1)]
        }
    }

    /// Takes one input sample and advances the moving average and the F0 estimate.
    fn push_sample(&mut self, x: f32) {
        let i = self.n as usize;
        self.history[i & (HISTORY - 1)] = x;
        self.smooth_sum += x;
        if i >= self.smooth_len {
            self.smooth_sum -= self.history[(i - self.smooth_len) & (HISTORY - 1)];
        }
        if i >= self.smooth_half {
            self.smooth[(i - self.smooth_half) & (HISTORY - 1)] = (self.smooth_sum / self.smooth_len as f32).abs();
        }
        let lp = self.lowpass.run(x);
        self.n += 1;
        if i % DECIMATION == DECIMATION - 1 {
            let d = self.decimated_count as usize;
            self.decimated[d & (DECIMATED - 1)] = lp;
            self.decimated_count += 1;
            if self.decimated_count as usize >= self.yin_window.len() && (d + 1).is_multiple_of(self.yin_hop) {
                self.estimate_f0();
            }
        }
    }

    /// Finds the F0 of the latest window (2 × the longest period) with YIN and writes it to the frame at the window's centre.
    fn estimate_f0(&mut self) {
        let len = self.yin_window.len();
        let end = self.decimated_count as usize;
        let mut energy = 0.0f32;
        for k in 0..len {
            let v = self.decimated[(end - len + k) & (DECIMATED - 1)];
            self.yin_window[k] = v;
            energy += v * v;
        }
        let rms = (energy / len as f32).sqrt();
        let f0 = if 20.0 * rms.max(1e-12).log10() < SILENCE_DBFS {
            self.yin_last = 0.0;
            self.yin_min = 1.0;
            0.0
        } else {
            self.yin()
        };
        let center = ((end - len / 2) * DECIMATION) as i64;
        let slot = center / self.unvoiced_hop as i64;
        // Spectral centroid of the frames around the window's centre. For a sine, power of the difference / power of the signal ≈ (2π f / fs)²
        let hop = self.unvoiced_hop as i64;
        let (mut e, mut ed) = (0.0f32, 0.0f32);
        for i in (center - hop).max(1)..center + hop {
            let (a, b) = (self.input_at(i), self.input_at(i - 1));
            e += a * a;
            ed += (a - b) * (a - b);
        }
        let centroid = if e > 0.0 { self.fs / (2.0 * std::f32::consts::PI) * (ed / e).sqrt() } else { 0.0 };
        if slot > self.last_slot {
            // Fill skipped frames with the same value
            let from = (self.last_slot + 1).max(slot - F0_SLOTS as i64 + 1);
            for s in from..=slot {
                self.f0_slots[s as usize & (F0_SLOTS - 1)] = f0;
                self.aperiodicity_slots[s as usize & (F0_SLOTS - 1)] = self.yin_min;
                self.centroid_slots[s as usize & (F0_SLOTS - 1)] = centroid;
            }
            // When voicing starts, fill the frames just before it that have no pitch marks yet with the same F0
            if f0 > 0.0 {
                let placed = if self.marks_len == 0 { -1 } else { self.mark(self.marks_len - 1).pos / self.unvoiced_hop as i64 };
                let mut s = from - 1;
                while s > placed && s >= slot - self.onset_backfill_slots && s >= 0 {
                    let k = s as usize & (F0_SLOTS - 1);
                    if self.f0_slots[k] > 0.0 {
                        break;
                    }
                    self.f0_slots[k] = f0;
                    s -= 1;
                }
            }
            self.last_slot = slot;
        }
    }

    fn yin(&mut self) -> f32 {
        let w = self.yin_tau_max;
        let x = &self.yin_window;
        let d = &mut self.yin_diff;
        d[0] = 1.0;
        let mut running = 0.0f32;
        for tau in 1..=w {
            let mut s = 0.0f32;
            for j in 0..w {
                let e = x[j] - x[j + tau];
                s += e * e;
            }
            running += s;
            d[tau] = if running > 0.0 { s * tau as f32 / running } else { 1.0 };
        }
        self.yin_min = d[self.yin_tau_min.max(2)..w].iter().copied().fold(f32::MAX, f32::min);
        let interpolate = |tau: usize| {
            let (a, b, c) = (d[tau - 1], d[tau], d[tau + 1]);
            let denom = a - 2.0 * b + c;
            let shift = if denom.abs() > 1e-12 { 0.5 * (a - c) / denom } else { 0.0 };
            tau as f32 + shift
        };
        let dfs = self.fs / DECIMATION as f32;
        let mut tau = self.yin_tau_min.max(2);
        while tau < w {
            if d[tau] < YIN_THRESHOLD {
                while tau + 1 < w && d[tau + 1] < d[tau] {
                    tau += 1;
                }
                let f0 = dfs / interpolate(tau);
                self.yin_last = f0;
                return f0;
            }
            tau += 1;
        }
        if self.yin_last > 0.0 {
            let center = dfs / self.yin_last;
            let lo = ((0.8 * center) as usize).max(self.yin_tau_min.max(2));
            let hi = ((1.2 * center).ceil() as usize).min(w - 1);
            if let Some(best) = (lo..=hi).min_by(|&a, &b| d[a].total_cmp(&d[b]))
                && d[best] < YIN_CONTINUE_THRESHOLD
                && best > 1
            {
                let f0 = dfs / interpolate(best);
                self.yin_last = f0;
                return f0;
            }
        }
        self.yin_last = 0.0;
        0.0
    }

    /// F0 at time `i`: the median of 3 with the neighbouring frames. Times not yet estimated use the newest value.
    fn f0_at(&self, i: i64) -> f32 {
        if self.last_slot < 0 {
            return 0.0;
        }
        let s = (i / self.unvoiced_hop as i64).min(self.last_slot).max(self.last_slot - F0_SLOTS as i64 + 2);
        let get = |k: i64| self.f0_slots[k as usize & (F0_SLOTS - 1)];
        let (a, b, c) = (get(s - 1), get(s), get((s + 1).min(self.last_slot)));
        a.max(b).min(a.min(b).max(c))
    }

    /// `f0_at` interpolated linearly between frame centres. The same as `f0_at` if either is unvoiced.
    fn f0_smooth_at(&self, i: i64) -> f32 {
        let hop = self.unvoiced_hop as i64;
        let left = (i - hop / 2).div_euclid(hop) * hop + hop / 2;
        let (a, b) = (self.f0_at(left), self.f0_at(left + hop));
        if a > 0.0 && b > 0.0 {
            a + (b - a) * (i - left) as f32 / hop as f32
        } else {
            self.f0_at(i)
        }
    }

    /// Whether the unvoiced frame at time `i` is aperiodic noise. Times not yet estimated use the newest frame.
    fn is_noise_at(&self, i: i64) -> bool {
        if self.last_slot < 0 {
            return false;
        }
        let s = (i / self.unvoiced_hop as i64).min(self.last_slot).max(self.last_slot - F0_SLOTS as i64 + 1);
        let k = s as usize & (F0_SLOTS - 1);
        let a = self.aperiodicity_slots[k];
        a > NOISE_APERIODICITY || (a > NOISE_APERIODICITY_HIGH_BAND && self.centroid_slots[k] > NOISE_MIN_FREQUENCY)
    }

    fn mark(&self, k: i64) -> Mark {
        self.marks[k as usize & (MARKS - 1)]
    }

    /// Places pitch marks as far as the received input allows, but only up to `horizon`.
    /// Placing them further than needed would settle word onsets as unvoiced before the F0 estimate can detect voicing.
    fn place_marks(&mut self, horizon: i64) {
        // The moving average is written up to n − 1 − smooth_half
        let available = self.n - 1 - self.smooth_half as i64;
        loop {
            let last = if self.marks_len == 0 {
                Mark { pos: -(self.unvoiced_hop as i64), period: self.unvoiced_hop, voiced: false, noise: false }
            } else {
                self.mark(self.marks_len - 1)
            };
            if last.pos >= horizon {
                break;
            }
            let guess = last.pos + last.period as i64;
            let f0 = self.f0_at(guess.max(0));
            let next = if f0 > 0.0 {
                let period = self.fs / f0;
                if last.voiced {
                    // Place it where the waveform best overlaps one period around the previous mark (0.8–1.2 periods ahead)
                    let lo = (0.8 * period) as i64;
                    let hi = (1.2 * period).ceil() as i64;
                    let half = (period / 2.0) as i64;
                    if last.pos + hi + half > self.n - 1 {
                        break;
                    }
                    let mut best = (period.round() as i64, f32::MIN);
                    for lag in lo..=hi {
                        let mut c = 0.0f32;
                        let mut e = 0.0f32;
                        for j in -half..half {
                            let b = self.input_at(last.pos + lag + j);
                            c += self.input_at(last.pos + j) * b;
                            e += b * b;
                        }
                        let score = c / (e.sqrt() + 1e-9);
                        if score > best.1 {
                            best = (lag, score);
                        }
                    }
                    // The correlation only sees the position relative to the previous mark, so once off the glottal pulse it does not come back. Half a period off,
                    // a grain holds two pulses of equal strength, and the original pitch is heard even when lowered. So move the mark onto the pulse (the maximum of the
                    // absolute moving average) only when the pulse within one period is clear and more than RESYNC_DISTANCE periods from the mark
                    let guess = last.pos + best.0;
                    let half_p = (period / 2.0) as i64;
                    let mut pos = guess;
                    if guess + half_p <= available {
                        let (mut q, mut qv) = (guess, -1.0f32);
                        for i in (guess - half_p).max(last.pos + 1)..guess + half_p {
                            let v = self.smooth[i as usize & (HISTORY - 1)];
                            if v > qv {
                                qv = v;
                                q = i;
                            }
                        }
                        let at_guess = self.smooth[guess as usize & (HISTORY - 1)];
                        if (q - guess).abs() as f32 > RESYNC_DISTANCE * period && qv > RESYNC_DOMINANCE * at_guess {
                            pos = q;
                        }
                    }
                    Mark { pos, period: (pos - last.pos) as f32, voiced: true, noise: false }
                } else {
                    // Where unvoiced turns voiced, place it at the maximum of the absolute moving average within one period
                    let (lo, hi) = (last.pos + 1, last.pos + 1 + period.ceil() as i64);
                    if hi > available {
                        break;
                    }
                    let mut best = lo.max(0);
                    let mut best_v = -1.0f32;
                    for p in lo.max(0)..hi {
                        let v = self.smooth[p as usize & (HISTORY - 1)];
                        if v > best_v {
                            best_v = v;
                            best = p;
                        }
                    }
                    Mark { pos: best, period, voiced: true, noise: false }
                }
            } else {
                let pos = last.pos + self.unvoiced_hop as i64;
                if pos > available {
                    break;
                }
                Mark { pos, period: self.unvoiced_hop, voiced: false, noise: self.is_noise_at(pos) }
            };
            self.marks[self.marks_len as usize & (MARKS - 1)] = next;
            self.marks_len += 1;
        }
    }

    /// The pitch mark nearest to time `t`.
    fn nearest_mark(&mut self, t: i64) -> Option<Mark> {
        if self.marks_len == 0 {
            return None;
        }
        let oldest = (self.marks_len - MARKS as i64 + 1).max(0);
        self.cursor = self.cursor.clamp(oldest, self.marks_len - 1);
        while self.cursor + 1 < self.marks_len && (self.mark(self.cursor + 1).pos - t).abs() <= (self.mark(self.cursor).pos - t).abs()
        {
            self.cursor += 1;
        }
        while self.cursor > oldest && (self.mark(self.cursor - 1).pos - t).abs() < (self.mark(self.cursor).pos - t).abs() {
            self.cursor -= 1;
        }
        Some(self.mark(self.cursor))
    }

    /// The earliest pitch mark at or after time `t`.
    fn next_mark_from(&self, t: i64) -> Option<Mark> {
        let oldest = (self.marks_len - MARKS as i64 + 1).max(0);
        (self.cursor.max(oldest)..self.marks_len).map(|k| self.mark(k)).find(|m| m.pos >= t)
    }

    /// Stretches the grain of input pitch mark `m` and overlaps it at synthesis mark `t`.
    fn add_grain(&mut self, t: i64, m: Mark) {
        let alpha = self.formant_ratio;
        let src_half = m.period.round().max(2.0);
        let half = ((src_half / alpha).round() as i64).clamp(2, self.max_half as i64);
        let len = 2 * half;
        let scale = if m.voiced { 1.0 } else { alpha };
        for u in -half..half {
            let src = m.pos as f32 + u as f32 * alpha;
            let i0 = src.floor();
            let frac = src - i0;
            let a = self.input_at(i0 as i64);
            let b = self.input_at(i0 as i64 + 1);
            let phase = (u + half) as f32 / len as f32;
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * phase).cos();
            if t + u < self.emitted {
                continue;
            }
            let idx = (t + u) as usize & (HISTORY - 1);
            let v = (a + (b - a) * frac) * w * scale;
            if m.voiced {
                self.accum[idx] += v;
            } else {
                self.accum_unvoiced[idx] += v;
            }
        }
    }
}

impl PsolaShifter {
    /// `process`, with unvoiced parts filled from `fallback` (the output of another shifter on the same input with the same delay).
    /// `fallback` has the same length as `input`.
    pub fn process_with_fallback(&mut self, input: &[f32], fallback: &[f32], output: &mut [f32]) {
        let start = self.advance(input);
        self.emit(start, input.len(), output, Some(fallback));
    }

    /// Takes input and places grains for the output to be finalised. Returns the input time before taking it.
    fn advance(&mut self, input: &[f32]) -> i64 {
        let start = self.n;
        for &x in input {
            self.push_sample(x);
        }
        // Output time o corresponds to input time o − latency. Place grains up to there, plus half a grain at the current period.
        // Placing ahead for the longest grain (60 Hz, formant ×0.5) would also settle pitch marks early and shorten what the F0 estimate sees ahead.
        let end = self.n - self.latency as i64;
        let f0 = self.f0_at(end.max(0));
        let period = if f0 > 0.0 { self.fs / f0 } else { self.unvoiced_hop };
        let half = ((period / self.formant_ratio).ceil() as usize + self.smooth_len).min(self.max_half) as i64;
        let limit = (end + half) as f64;
        // Place marks ahead by the longest period so a pitch mark near each synthesis mark can be chosen
        let longest_period = (self.fs / MIN_F0).ceil() as i64;
        self.place_marks(end + half + longest_period);
        while self.next_synthesis < limit {
            let from = self.next_synthesis as i64;
            let mut t = from;
            let Some(mut m) = self.nearest_mark(t) else { break };
            // The first voiced grain is aligned with the following pitch mark, removing the time offset from the input
            if m.voiced
                && !self.last_voiced
                && let Some(next) = self.next_mark_from(t)
                && next.voiced
            {
                t = next.pos;
                m = next;
                self.next_synthesis = t as f64;
            }
            self.last_voiced = m.voiced;
            self.add_grain(t, m);
            // The voiced grain interval comes from the F0 period at that time, not from the spacing of pitch marks (which jitters by whole samples)
            let hop = if m.voiced {
                let f0 = self.f0_smooth_at(m.pos);
                let period = if f0 > 0.0 { self.fs / f0 } else { m.period };
                period / self.pitch_ratio
            } else {
                self.unvoiced_hop
            };
            self.next_synthesis += hop.max(1.0) as f64;
            // Gaps left by realignment are unvoiced; the span a grain covers takes that grain's voicing
            let until = self.next_synthesis as i64;
            for i in from.max(0)..until.max(0) {
                let k = i as usize & (HISTORY - 1);
                self.voiced_flag[k] = if i >= t && m.voiced { 1.0 } else { 0.0 };
                self.noise_flag[k] = if i >= t && m.noise { 1.0 } else { 0.0 };
            }
        }
        start
    }

    fn emit(&mut self, start: i64, len: usize, output: &mut [f32], fallback: Option<&[f32]>) {
        for (k, y) in output.iter_mut().enumerate().take(len) {
            let t = start + k as i64 - self.latency as i64;
            if t < 0 {
                *y = fallback.map_or(0.0, |f| f[k]);
                continue;
            }
            let idx = t as usize & (HISTORY - 1);
            *y = match fallback {
                None => self.accum[idx] + self.accum_unvoiced[idx],
                Some(f) => {
                    self.voiced_smooth += (self.voiced_flag[idx] - self.voiced_smooth) * self.voiced_coef;
                    self.noise_smooth += (self.noise_flag[idx] - self.noise_smooth) * self.voiced_coef;
                    let fallback = (1.0 - self.voiced_smooth - self.noise_smooth).max(0.0);
                    self.accum[idx] + self.noise_smooth * self.accum_unvoiced[idx] + fallback * f[k]
                }
            };
            self.accum[idx] = 0.0;
            self.accum_unvoiced[idx] = 0.0;
        }
        self.emitted = start + len as i64 - self.latency as i64;
    }
}

impl VoiceShifter for PsolaShifter {
    fn set(&mut self, settings: ShiftSettings) {
        self.pitch_ratio = 2f32.powf(settings.pitch.semitones() / 12.0);
        self.formant_ratio = settings.formant.ratio().max(MIN_FORMANT);
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let start = self.advance(input);
        self.emit(start, input.len(), output, None);
    }

    fn latency(&self) -> usize {
        self.latency
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yin::Yin;
    use voice_domain::ports::PitchEstimator;
    use voice_domain::{FormantShift, PitchShift, SampleRate};

    const FS: u32 = 48_000;

    /// A vowel-like signal: harmonics falling as 1/k, coloured by two resonances.
    fn vowel(f0: f32, seconds: f32) -> Vec<f32> {
        let n = (seconds * FS as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / FS as f32;
                (1..40)
                    .map(|k| {
                        let f = k as f32 * f0;
                        let g = 1.0 / (1.0 + ((f - 700.0) / 150.0).powi(2)) + 0.5 / (1.0 + ((f - 1200.0) / 200.0).powi(2));
                        g * (2.0 * std::f32::consts::PI * f * t).sin()
                    })
                    .sum::<f32>()
                    * 0.1
            })
            .collect()
    }

    fn run(s: &mut PsolaShifter, x: &[f32]) -> Vec<f32> {
        let mut y = vec![0.0; x.len()];
        for (i, o) in x.chunks(480).zip(y.chunks_mut(480)) {
            s.process(i, o);
        }
        y
    }

    fn settings(st: f32, formant: f32) -> ShiftSettings {
        ShiftSettings { pitch: PitchShift::new(st).unwrap(), formant: FormantShift::new(formant).unwrap() }
    }

    fn f0_of(y: &[f32]) -> f32 {
        let mut yin = Yin::default();
        yin.estimate(y, SampleRate::new(FS).unwrap()).expect("voiced").hz()
    }

    #[test]
    fn reports_the_latency_it_was_built_with() {
        assert_eq!(PsolaShifter::new(FS, 3360).latency(), 3360);
    }

    #[test]
    fn lowers_the_pitch_by_an_octave() {
        let mut s = PsolaShifter::new(FS, 3360);
        s.set(settings(-12.0, 1.0));
        let y = run(&mut s, &vowel(150.0, 1.5));
        assert!(y.iter().all(|v| v.is_finite()));
        let f0 = f0_of(&y[FS as usize..FS as usize + 4096]);
        assert!((f0 / 75.0 - 1.0).abs() < 0.01, "{f0}");
    }

    #[test]
    fn keeps_the_pitch_when_only_the_formant_moves() {
        let mut s = PsolaShifter::new(FS, 3360);
        s.set(settings(0.0, 0.8));
        let y = run(&mut s, &vowel(120.0, 1.5));
        let f0 = f0_of(&y[FS as usize..FS as usize + 4096]);
        assert!((f0 / 120.0 - 1.0).abs() < 0.01, "{f0}");
    }

    #[test]
    fn output_is_delayed_by_the_latency() {
        // With no change, the envelope (RMS every 1 ms) of a vowel with slowly changing amplitude comes out shifted by the delay.
        // A periodic waveform overlaps itself even one period off, so compare the envelopes.
        let mut s = PsolaShifter::new(FS, 3360);
        s.set(ShiftSettings::NONE);
        let x: Vec<f32> = vowel(150.0, 2.0)
            .iter()
            .enumerate()
            .map(|(i, v)| v * (1.0 + 0.8 * (2.0 * std::f32::consts::PI * 2.7 * i as f32 / FS as f32).sin()))
            .collect();
        let y = run(&mut s, &x);
        let env = |v: &[f32]| -> Vec<f32> { v.chunks(48).map(|c| c.iter().map(|a| a * a).sum::<f32>().sqrt()).collect() };
        let (ex, ey) = (env(&x), env(&y));
        let lag = (40..100)
            .max_by(|&a, &b| {
                let c = |l: usize| (200..1800).map(|i| ey[i] * ex[i - l]).sum::<f32>();
                c(a).total_cmp(&c(b))
            })
            .unwrap();
        assert!((lag as i64 * 48 - 3360).abs() <= 96, "{} samples", lag * 48);
    }

    /// Uniform noise with a fixed seed.
    fn noise(len: usize, amplitude: f32) -> Vec<f32> {
        let mut state = 1u32;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state as f32 / u32::MAX as f32 - 0.5) * 2.0 * amplitude
            })
            .collect()
    }

    /// The mean output after 0.5 s when `process_with_fallback` is given a constant 0.5 output (half the share of the given output mixed in).
    fn fallback_share(x: &[f32]) -> f32 {
        let fallback = vec![0.5f32; 480];
        let mut s = PsolaShifter::new(FS, 3360);
        s.set(settings(-12.0, 1.0));
        let mut y = vec![0.0; x.len()];
        for (i, o) in x.chunks(480).zip(y.chunks_mut(480)) {
            s.process_with_fallback(i, &fallback[..i.len()], o);
        }
        let tail = &y[FS as usize / 2..];
        tail.iter().sum::<f32>() / tail.len() as f32 / 0.5
    }

    #[test]
    fn fills_unvoiced_parts_with_the_fallback_only_when_some_periodicity_remains() {
        // Not mixed into a vowel (voiced)
        assert!(fallback_share(&vowel(150.0, 1.0)).abs() < 0.02);
        // Noise alone (aperiodic unvoiced) is not mixed; unvoiced grains are output as they are
        assert!(fallback_share(&noise(FS as usize, 0.01)).abs() < 0.02);
        // A vowel buried in low-frequency noise (judged unvoiced but still periodic, with a low centroid; this is missed voicing)
        // is filled with the given output
        let mut low = 0.0f32;
        let buried: Vec<f32> = vowel(150.0, 1.0)
            .iter()
            .zip(noise(FS as usize, 0.3))
            .map(|(v, n)| {
                low += (n - low) * 0.05;
                v * 0.3 + low
            })
            .collect();
        let share = fallback_share(&buried);
        assert!(share > 0.9, "{share}");
    }

    #[test]
    fn silence_stays_silent() {
        let mut s = PsolaShifter::new(FS, 3360);
        s.set(settings(-12.0, 0.8));
        let y = run(&mut s, &vec![0.0; FS as usize]);
        assert!(y.iter().all(|v| *v == 0.0));
    }
}
