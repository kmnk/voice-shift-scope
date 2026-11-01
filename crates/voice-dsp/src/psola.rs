//! TD-PSOLA で声を変える `PsolaShifter`。
//!
//! 入力の F0 の周期ごとにピッチマーク（基準点）を置き、2 周期の Hann 窓の粒を、新しい周期の間隔で重ねる。
//! フォルマントは粒を時間方向に `1 / 倍率` に伸縮して動かす。無声の間は 5 ms ごとの粒を同じ間隔で重ねる
//! （無声の粒も伸縮する）。
//!
//! `process_with_fallback` では、無声と判定した区間のうち、周期性がいくらか残る所（有声の取りこぼしの見込み）を、
//! 渡された別の処理の出力（同じ遅れのもの）で埋める。無声の粒はピッチを動かさないので、有声の音を無声と取りこぼすと
//! 元の高さの声が混ざる。周期性がほぼ無い所（さしすせその子音などの息の雑音）は、無声の粒のまま（ピッチを動かさず、
//! フォルマントだけ動かして）出す。雑音には音程が無く、別の処理でピッチを下げると高い成分が下がってこもる。
//!
//! 遅れは作るときに決める（`HybridShifter` が Signalsmith Stretch と同じ値を渡す）。入力の時刻 n までが届いたら、
//! 時刻 n − 遅れ までの出力を確定させる。F0 の推定、ピッチマークの探索、粒の切り出しの先読みは、遅れの中に収める。
//! 入力の F0 は 60〜800 Hz、フォルマントの倍率は 0.5〜2.0 を前提にし、遅れは 70 ms 以上を要る。
//!
//! 作るときにすべての buffer を確保し、`process` の中では確保しない。

use voice_domain::ShiftSettings;
use voice_domain::ports::VoiceShifter;

const MIN_F0: f32 = 60.0;
const MAX_F0: f32 = 800.0;
const MIN_FORMANT: f32 = 0.5;
/// 無声の粒の間隔（5 ms）。粒の長さはこの 2 倍。
const UNVOICED_HOP_SECONDS: f32 = 0.005;
/// F0 を求める信号の間引き（48 kHz → 12 kHz）。
const DECIMATION: usize = 4;
/// ピッチマークを拾う信号の移動平均の長さ（1 ms）。
const SMOOTH_SECONDS: f32 = 0.001;
/// YIN の累積平均で正規化した差のしきい値。有声の始まりは厳しく、有声が続いている間は前の周期の ±20% の範囲を
/// 緩いしきい値で探す（揺れや息の雑音で無声に落ちるのを防ぎ、倍音を F0 と取り違えるのは始まりで防ぐ）。
const YIN_THRESHOLD: f32 = 0.15;
/// ピッチマークを声門の脈に置き直す条件: マークからの距離（周期に対する割合）と、脈の山とマークの所の比。
/// 距離 0.25 では Kyoko の「今日は」で奇数と偶数の倍音の差が −1.7 dB、0.15 では −1.1 dB（PSOLA 試作 −0.9 dB）だった。
const RESYNC_DISTANCE: f32 = 0.15;
const RESYNC_DOMINANCE: f32 = 1.5;
const YIN_CONTINUE_THRESHOLD: f32 = 0.4;
/// 無声の枠のうち、YIN の累積平均で正規化した差の最小が NOISE_APERIODICITY より大きい所か、
/// NOISE_APERIODICITY_HIGH_BAND より大きく、強さの重心（差分の強さと元の強さの比から求める）が
/// NOISE_MIN_FREQUENCY より上の所を、周期性の無い雑音とみなす（しきい値は読み上げの声 2 つで Q10 と Q13 を両方満たす組から選んだ）。重心を見るのは、取りこぼした有声の音（低い周波数に偏る）と、
/// さしすせその子音（高い周波数に偏る）を分けるため。周期性だけで分けると、しきい値を 0.4 に下げた所で Daniel の
/// −12 半音で元の高さのまま出る枠が 4.9% になり、0.5 では子音の一部を別の処理で下げて重心が 12% 下がった。
const NOISE_APERIODICITY: f32 = 0.5;
const NOISE_APERIODICITY_HIGH_BAND: f32 = 0.3;
const NOISE_MIN_FREQUENCY: f32 = 5000.0;
/// 無声とみなす RMS。
const SILENCE_DBFS: f32 = -50.0;

/// 有声を見つけたとき、まだピッチマークを置いていない直前の何秒を同じ F0 の有声として埋め戻すか。
/// YIN の窓（約 33 ms）は言葉の頭で半分しか有声を含まず、有声と判定できるのが遅れるため。
const ONSET_BACKFILL_SECONDS: f32 = 0.025;

/// 有声・無声の切り替わりで、別の処理の出力と混ぜる重みを滑らかにする時定数。
const VOICED_SMOOTH_SECONDS: f32 = 0.005;

const HISTORY: usize = 1 << 14;
const MARKS: usize = 1 << 10;
const F0_SLOTS: usize = 1 << 10;
const DECIMATED: usize = 1 << 11;

/// 2 次の低域通過（RBJ）。
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
    /// 周期（標本）。無声のときは無声の粒の間隔。
    period: f32,
    voiced: bool,
    /// 無声のうち、周期性の無い雑音か。
    noise: bool,
}

pub struct PsolaShifter {
    fs: f32,
    latency: usize,
    pitch_ratio: f32,
    formant_ratio: f32,
    /// 届いた入力の標本の数（次の入力の時刻）。
    n: i64,
    history: Vec<f32>,
    /// 中心の 1 ms の移動平均（`smooth_half` 標本遅れて書く）。
    smooth: Vec<f32>,
    smooth_sum: f32,
    smooth_len: usize,
    smooth_half: usize,
    // F0 の推定
    lowpass: Biquad,
    decimated: Vec<f32>,
    decimated_count: i64,
    yin_hop: usize,
    yin_tau_min: usize,
    yin_tau_max: usize,
    yin_window: Vec<f32>,
    yin_diff: Vec<f32>,
    /// 直前の推定の F0（0 は無声）。
    yin_last: f32,
    /// 5 ms ごとの F0（0 は無声）。添字は「時刻 / 無声の間隔」。
    f0_slots: Vec<f32>,
    /// 枠ごとの、YIN の累積平均で正規化した差の最小（大きいほど周期性が無い）。
    aperiodicity_slots: Vec<f32>,
    /// 枠ごとの、強さの重心（Hz）。
    centroid_slots: Vec<f32>,
    yin_min: f32,
    last_slot: i64,
    unvoiced_hop: f32,
    // ピッチマーク
    marks: Vec<Mark>,
    marks_len: i64,
    cursor: i64,
    /// 確定させて出した出力の、次の入力の時刻。これより前には粒を足さない。
    emitted: i64,
    onset_backfill_slots: i64,
    // 合成
    next_synthesis: f64,
    last_voiced: bool,
    max_half: usize,
    /// 有声の粒と無声の粒を重ねる先。添字は入力の時刻。
    accum: Vec<f32>,
    accum_unvoiced: Vec<f32>,
    /// 合成の時刻ごとの有声（1）・無声（0）。`process_with_fallback` で別の処理の出力と混ぜる重みに使う。
    voiced_flag: Vec<f32>,
    voiced_smooth: f32,
    /// 合成の時刻ごとの、周期性の無い無声（1）か。その所は別の処理で埋めず、無声の粒を出す。
    noise_flag: Vec<f32>,
    noise_smooth: f32,
    voiced_coef: f32,
}

// 1 つのスレッドからしか触らない。
unsafe impl Send for PsolaShifter {}

impl PsolaShifter {
    /// `latency` は入力から出力までの遅れ（標本）。70 ms より短いと先読みが足りない。
    /// 作るときにメモリを確保するので、音声のスレッドの外で作る。
    pub fn new(sample_rate: u32, latency: usize) -> Self {
        let fs = sample_rate as f32;
        assert!(latency as f32 >= 0.07 * fs, "PSOLA の遅れは 70 ms 以上が要る");
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

    /// 入力の 1 標本を受け取り、移動平均と F0 の推定を進める。
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

    /// 直近の窓（2 × 最長の周期）の F0 を YIN で求め、窓の中心の時刻の枠に書く。
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
        // 窓の中心の前後 1 枠の、強さの重心。正弦波では、差分の強さ / 元の強さ ≈ (2π f / fs)²
        let hop = self.unvoiced_hop as i64;
        let (mut e, mut ed) = (0.0f32, 0.0f32);
        for i in (center - hop).max(1)..center + hop {
            let (a, b) = (self.input_at(i), self.input_at(i - 1));
            e += a * a;
            ed += (a - b) * (a - b);
        }
        let centroid = if e > 0.0 { self.fs / (2.0 * std::f32::consts::PI) * (ed / e).sqrt() } else { 0.0 };
        if slot > self.last_slot {
            // 飛んだ枠は同じ値で埋める
            let from = (self.last_slot + 1).max(slot - F0_SLOTS as i64 + 1);
            for s in from..=slot {
                self.f0_slots[s as usize & (F0_SLOTS - 1)] = f0;
                self.aperiodicity_slots[s as usize & (F0_SLOTS - 1)] = self.yin_min;
                self.centroid_slots[s as usize & (F0_SLOTS - 1)] = centroid;
            }
            // 有声が始まったら、まだピッチマークを置いていない直前の枠を同じ F0 で埋め戻す
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

    /// 時刻 `i` の F0。前後の枠と合わせた 3 つの中央値。まだ求めていない時刻は一番新しい値を使う。
    fn f0_at(&self, i: i64) -> f32 {
        if self.last_slot < 0 {
            return 0.0;
        }
        let s = (i / self.unvoiced_hop as i64).min(self.last_slot).max(self.last_slot - F0_SLOTS as i64 + 2);
        let get = |k: i64| self.f0_slots[k as usize & (F0_SLOTS - 1)];
        let (a, b, c) = (get(s - 1), get(s), get((s + 1).min(self.last_slot)));
        a.max(b).min(a.min(b).max(c))
    }

    /// `f0_at` を、枠の中心どうしの間で直線で補ったもの。どちらかが無声なら `f0_at` と同じ。
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

    /// 時刻 `i` の無声の枠が、周期性の無い雑音か。まだ求めていない時刻は一番新しい枠で決める。
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

    /// 届いた入力で決められるところまで、ただし `horizon` までピッチマークを置く。
    /// 要る所より先に置くと、言葉の頭で F0 の推定が有声と判定できる前に無声として決まってしまう。
    fn place_marks(&mut self, horizon: i64) {
        // 移動平均が書かれているのは n − 1 − smooth_half まで
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
                    // 前のマークの周り 1 周期の波形と一番よく重なる位置（0.8〜1.2 周期先）に置く
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
                    // 相関は前のマークとの相対の位置しか見ないので、声門の脈からずれると戻らない。半周期ずれると
                    // 粒に脈が 2 つ同じ強さで入り、下げても元の高さが聞こえる。そこで、1 周期の中で脈（移動平均の
                    // 絶対値の最大）がはっきりしていて、マークから RESYNC_DISTANCE 周期より離れているときだけ、脈に置き直す
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
                    // 無声から有声に変わったところは、1 周期の中で移動平均の絶対値が最大の所に置く
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

    /// 時刻 `t` に一番近いピッチマーク。
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

    /// 時刻 `t` 以後で一番早いピッチマーク。
    fn next_mark_from(&self, t: i64) -> Option<Mark> {
        let oldest = (self.marks_len - MARKS as i64 + 1).max(0);
        (self.cursor.max(oldest)..self.marks_len).map(|k| self.mark(k)).find(|m| m.pos >= t)
    }

    /// 合成のマーク `t` に、入力のピッチマーク `m` の粒を伸縮して重ねる。
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
    /// `process` の、無声の区間を `fallback`（同じ入力を同じ遅れで変えた別の処理の出力）で埋めるもの。
    /// `fallback` は `input` と同じ長さ。
    pub fn process_with_fallback(&mut self, input: &[f32], fallback: &[f32], output: &mut [f32]) {
        let start = self.advance(input);
        self.emit(start, input.len(), output, Some(fallback));
    }

    /// 入力を受け取り、確定させる出力の分まで粒を置く。返すのは、受け取る前の入力の時刻。
    fn advance(&mut self, input: &[f32]) -> i64 {
        let start = self.n;
        for &x in input {
            self.push_sample(x);
        }
        // 出力の時刻 o は入力の時刻 o − latency に当たる。そこまでの粒を、今の周期での粒の半分の長さだけ先まで置く。
        // 最長の粒（60 Hz・フォルマント 0.5 倍）の分まで先に置くと、ピッチマークも先に決まり、F0 の推定が見る先が短くなる。
        let end = self.n - self.latency as i64;
        let f0 = self.f0_at(end.max(0));
        let period = if f0 > 0.0 { self.fs / f0 } else { self.unvoiced_hop };
        let half = ((period / self.formant_ratio).ceil() as usize + self.smooth_len).min(self.max_half) as i64;
        let limit = (end + half) as f64;
        // 合成のマークの近くのピッチマークを選べるよう、最長の周期の分だけ先まで置く
        let longest_period = (self.fs / MIN_F0).ceil() as i64;
        self.place_marks(end + half + longest_period);
        while self.next_synthesis < limit {
            let from = self.next_synthesis as i64;
            let mut t = from;
            let Some(mut m) = self.nearest_mark(t) else { break };
            // 有声になった最初の粒は、その後のピッチマークに合わせて置き、入力との時刻のずれを無くす
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
            // 有声の粒の間隔は、ピッチマークの間（整数の標本で揺れる）でなく、その時刻の F0 の周期から決める
            let hop = if m.voiced {
                let f0 = self.f0_smooth_at(m.pos);
                let period = if f0 > 0.0 { self.fs / f0 } else { m.period };
                period / self.pitch_ratio
            } else {
                self.unvoiced_hop
            };
            self.next_synthesis += hop.max(1.0) as f64;
            // 置き直しで空いた所は無声、粒の受け持つ所はその粒の有声・無声
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

    /// 1/k で弱まる倍音を並べ、2 つの共鳴で色を付けた母音のような信号。
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
        yin.estimate(y, SampleRate::new(FS).unwrap()).expect("有声").hz()
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
        // 何も変えないとき、振幅をゆっくり変えた母音の包絡（1 ms ごとの RMS）が、遅れの分だけずれて出る。
        // 周期的な波形そのものは 1 周期ずれても重なるので、包絡で比べる。
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
        assert!((lag as i64 * 48 - 3360).abs() <= 96, "{} 標本", lag * 48);
    }

    /// 決まった種の一様な雑音。
    fn noise(len: usize, amplitude: f32) -> Vec<f32> {
        let mut state = 1u32;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state as f32 / u32::MAX as f32 - 0.5) * 2.0 * amplitude
            })
            .collect()
    }

    /// `process_with_fallback` に値 0.5 の一定の出力を渡したときの、0.5 秒から後の出力の平均（渡した出力が混ざった割合の半分）。
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
        // 母音（有声）には混ぜない
        assert!(fallback_share(&vowel(150.0, 1.0)).abs() < 0.02);
        // 雑音だけ（周期性の無い無声）は混ぜず、無声の粒をそのまま出す
        assert!(fallback_share(&noise(FS as usize, 0.01)).abs() < 0.02);
        // 低い周波数の雑音に埋もれた母音（無声と判定するが周期性が残り、強さの重心が低い。有声の取りこぼしに当たる）は
        // 渡した出力で埋める
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
