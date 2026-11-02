//! F1〜F4 の追跡。フレームごとの LPC の根を候補にし、前のフレームとのつながりを見て F1〜F4 に割り当て、
//! 直近 5 フレームの中央値を返す。
//!
//! 低い方から 4 つを取るだけだと、共鳴でない根（帯域幅の広い根など）が 1 つ混ざったフレームで番号が 1 つずれ、
//! 点が別の帯に飛ぶ。割り当ての費用は Praat の Formant: Track... と同じ形（参照の周波数からの離れ、帯域幅/周波数、
//! 隣のフレームとの変化）で、先を見ずに今のフレームで決める。

use std::collections::VecDeque;

use voice_domain::{Formant, Frequency};

/// 返す F1〜F4 が何フレーム前のものか（中央値の窓の後ろ半分）。分析が 10 ms ごとなら 20 ms。
pub const FORMANT_DELAY_FRAMES: usize = 2;
const MEDIAN_FRAMES: usize = 2 * FORMANT_DELAY_FRAMES + 1;
/// 割り当ての参照の周波数（Praat の Formant: Track... の説明にある値）。上限が既定（5500 Hz）のときの値で、
/// 上限に比例させて使う。上限は声の共鳴の高さに合わせて選ぶもので、フォルマントを変えた後の声は上限を倍率で上げて
/// 分析するため。固定のままだと、フォルマントを 0.8 倍にした声で F3 を F2 の枠に入れた。
const REFERENCES: [f64; 4] = [550.0, 1650.0, 2750.0, 3850.0];
const REFERENCE_CEILING: f64 = 5500.0;
/// 空の枠の費用（候補が 4 つより少ないときだけ使う）。
const EMPTY_COST: f64 = 3.0;

/// 候補の添字を F1〜F4 に割り当てたもの。`None` は空の枠。
type Assignment = [Option<usize>; 4];
/// 1 フレームの候補と、割り当てごとの累積の費用。
type Frame = (Vec<Formant>, Vec<(Assignment, f64)>);

#[derive(Default)]
pub struct FormantTracker {
    /// 前のフレームの候補と、割り当てごとの累積の費用
    previous: Option<Frame>,
    /// 直近の割り当ての結果（新しいものが後ろ）
    recent: VecDeque<[Option<Formant>; 4]>,
}

impl FormantTracker {
    /// 有声のフレームの候補（周波数の順）と分析の上限（Hz）を渡し、`FORMANT_DELAY_FRAMES` 前のフレームの F1〜F4 を受け取る。
    /// 有声の始まりでまだ足りないうちは、持っているフレームだけで出す。
    pub fn push(&mut self, candidates: &[Formant], ceiling: f64) -> [Option<Formant>; 4] {
        let scale = ceiling / REFERENCE_CEILING;
        let assignments = assignments(candidates.len());
        let local: Vec<f64> = assignments.iter().map(|a| local_cost(a, candidates, scale)).collect();
        let costs: Vec<(Assignment, f64)> = match &self.previous {
            None => assignments.iter().copied().zip(local).collect(),
            Some((prev_cands, prev)) => assignments
                .iter()
                .zip(local)
                .map(|(a, l)| {
                    let best = prev
                        .iter()
                        .map(|(p, c)| c + transition_cost(p, prev_cands, a, candidates))
                        .fold(f64::INFINITY, f64::min);
                    (*a, best + l)
                })
                .collect(),
        };
        // 累積の費用が大きくなり続けないように、最小を引く
        let min = costs.iter().map(|(_, c)| *c).fold(f64::INFINITY, f64::min);
        let costs: Vec<(Assignment, f64)> = costs.into_iter().map(|(a, c)| (a, c - min)).collect();
        let best = costs.iter().min_by(|x, y| x.1.total_cmp(&y.1)).map(|(a, _)| *a).unwrap_or([None; 4]);
        self.recent.push_back(best.map(|i| i.map(|i| candidates[i])));
        if self.recent.len() > MEDIAN_FRAMES {
            self.recent.pop_front();
        }
        self.previous = Some((candidates.to_vec(), costs));
        self.smoothed()
    }

    /// 無声のフレームで呼ぶ。つながりを切る。
    pub fn reset(&mut self) {
        self.previous = None;
        self.recent.clear();
    }

    /// 後ろから `FORMANT_DELAY_FRAMES + 1` 番目のフレームの、前後 `FORMANT_DELAY_FRAMES` フレーム（持っている範囲）の中央値。
    fn smoothed(&self) -> [Option<Formant>; 4] {
        let n = self.recent.len();
        let target = n.saturating_sub(FORMANT_DELAY_FRAMES + 1);
        let lo = target.saturating_sub(FORMANT_DELAY_FRAMES);
        let hi = (target + FORMANT_DELAY_FRAMES).min(n - 1);
        let mut out = [None; 4];
        for (k, slot) in out.iter_mut().enumerate() {
            if self.recent[target][k].is_none() {
                continue;
            }
            let values: Vec<Formant> = (lo..=hi).filter_map(|t| self.recent[t][k]).collect();
            let f = median(values.iter().map(|v| v.frequency.hz()).collect());
            let b = median(values.iter().map(|v| v.bandwidth.hz()).collect());
            if let (Ok(frequency), Ok(bandwidth)) = (Frequency::new(f), Frequency::new(b)) {
                *slot = Some(Formant { frequency, bandwidth });
            }
        }
        out
    }
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// 候補 `n` 個から F1〜F4 への、周波数の順を保つ割り当てを全部並べる。
/// 候補が 4 つ以上なら 4 枠すべてを埋めるものだけ、4 つより少なければ空の枠を許す。
fn assignments(n: usize) -> Vec<Assignment> {
    let k = n.min(4);
    let mut out = Vec::new();
    for chosen in combinations(n, k) {
        for slots in combinations(4, k) {
            let mut a = [None; 4];
            for (c, s) in chosen.iter().zip(&slots) {
                a[*s] = Some(*c);
            }
            out.push(a);
        }
    }
    out
}

/// 0..n から k 個を選ぶ組み合わせ（昇順）。
fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn go(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..n {
            cur.push(i);
            go(i + 1, n, k, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    go(0, n, k, &mut Vec::new(), &mut out);
    out
}

fn local_cost(a: &Assignment, candidates: &[Formant], scale: f64) -> f64 {
    a.iter()
        .zip(REFERENCES.map(|r| r * scale))
        .map(|(slot, reference)| match slot {
            None => EMPTY_COST,
            Some(i) => {
                let f = candidates[*i].frequency.hz() as f64;
                let b = candidates[*i].bandwidth.hz() as f64;
                (f - reference).abs() / reference + b / f
            }
        })
        .sum()
}

fn transition_cost(prev: &Assignment, prev_cands: &[Formant], cur: &Assignment, candidates: &[Formant]) -> f64 {
    prev.iter()
        .zip(cur)
        .map(|(p, c)| match (p, c) {
            (Some(p), Some(c)) => {
                let a = prev_cands[*p].frequency.hz() as f64;
                let b = candidates[*c].frequency.hz() as f64;
                (a - b).abs() / (a + b)
            }
            _ => 0.0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(f: f32, bw: f32) -> Formant {
        Formant { frequency: Frequency::new(f).unwrap(), bandwidth: Frequency::new(bw).unwrap() }
    }

    fn hz(out: [Option<Formant>; 4]) -> [Option<f32>; 4] {
        out.map(|f| f.map(|f| f.frequency.hz()))
    }

    fn steady() -> Vec<Formant> {
        vec![fm(700.0, 80.0), fm(1200.0, 90.0), fm(2600.0, 120.0), fm(3500.0, 150.0), fm(4500.0, 200.0)]
    }

    /// F2 と F3 の間に帯域幅の広い根が 1 フレームだけ混ざっても、F3・F4 は本当の共鳴のまま
    #[test]
    fn a_spurious_root_does_not_shift_the_numbers() {
        let mut t = FormantTracker::default();
        let mut outs = Vec::new();
        for frame in 0..12 {
            let mut c = steady();
            if frame == 6 {
                c.insert(2, fm(1900.0, 900.0));
                c.pop();
            }
            outs.push(hz(t.push(&c, 5500.0)));
        }
        for o in &outs {
            assert_eq!(*o, [Some(700.0), Some(1200.0), Some(2600.0), Some(3500.0)], "{outs:?}");
        }
    }

    /// 割り当てだけ（中央値の前）でも、偽の根のフレームで番号がずれない
    #[test]
    fn the_assignment_skips_a_spurious_root() {
        let mut t = FormantTracker::default();
        for _ in 0..3 {
            t.push(&steady(), 5500.0);
        }
        let mut c = steady();
        c.insert(2, fm(1900.0, 900.0));
        c.pop();
        t.push(&c, 5500.0);
        let latest = t.recent.back().unwrap().map(|f| f.map(|f| f.frequency.hz()));
        assert_eq!(latest, [Some(700.0), Some(1200.0), Some(2600.0), Some(3500.0)]);
    }

    /// 1 フレームだけの外れは中央値で消え、値は FORMANT_DELAY_FRAMES 遅れて出る
    #[test]
    fn the_median_removes_a_single_outlier_and_delays_by_two_frames() {
        let mut t = FormantTracker::default();
        let f2 = [1200.0, 1200.0, 1200.0, 1500.0, 1200.0, 1200.0, 1200.0, 1800.0, 1800.0, 1800.0, 1800.0];
        let outs: Vec<f32> = f2
            .iter()
            .map(|v| {
                let c = vec![fm(700.0, 80.0), fm(*v, 90.0), fm(2600.0, 120.0), fm(3500.0, 150.0)];
                t.push(&c, 5500.0)[1].unwrap().frequency.hz()
            })
            .collect();
        assert!(outs.iter().all(|v| *v != 1500.0), "{outs:?}");
        // 1800 に変わったのは添字 7 のフレーム。中心の中央値は段差の位置を保ち、2 フレーム遅れて添字 9 で出る
        assert_eq!(outs[8], 1200.0, "{outs:?}");
        assert_eq!(outs[9], 1800.0, "{outs:?}");
    }

    /// 無声で戻すと、前の声の値を引きずらない
    #[test]
    fn reset_forgets_the_previous_voice() {
        let mut t = FormantTracker::default();
        for _ in 0..5 {
            t.push(&steady(), 5500.0);
        }
        t.reset();
        let c = vec![fm(400.0, 60.0), fm(2000.0, 90.0), fm(2900.0, 120.0), fm(3800.0, 150.0)];
        assert_eq!(hz(t.push(&c, 5500.0)), [Some(400.0), Some(2000.0), Some(2900.0), Some(3800.0)]);
    }

    /// 上限を下げて分析した声（フォルマントを 0.8 倍にした声）では、参照も下がり、下がった F2 を F2 の枠に入れる
    #[test]
    fn references_follow_the_ceiling() {
        let mut t = FormantTracker::default();
        let c = vec![fm(560.0, 80.0), fm(960.0, 90.0), fm(2080.0, 120.0), fm(2800.0, 150.0), fm(3600.0, 200.0)];
        assert_eq!(hz(t.push(&c, 4400.0)), [Some(560.0), Some(960.0), Some(2080.0), Some(2800.0)]);
    }

    /// 候補が 4 つより少なければ、参照に近い枠に入れ、残りを空にする
    #[test]
    fn fewer_candidates_leave_empty_slots() {
        let mut t = FormantTracker::default();
        let c = vec![fm(700.0, 80.0), fm(1200.0, 90.0), fm(2600.0, 120.0)];
        assert_eq!(hz(t.push(&c, 5500.0)), [Some(700.0), Some(1200.0), Some(2600.0), None]);
    }
}
