//! F1–F4 tracking. Takes the LPC roots of each frame as candidates, assigns them to F1–F4 by their continuity with
//! the previous frame, and returns the median of the last 5 frames.
//!
//! Just taking the 4 lowest roots shifts the numbering by one in frames where one root that is not a resonance (such as
//! a root with a wide bandwidth) gets in, and the dot jumps to another band. The assignment cost has the same form as Praat's
//! Formant: Track... (distance from reference frequencies, bandwidth/frequency, change from the neighbouring frame), decided at the current frame without lookahead.

use std::collections::VecDeque;

use voice_domain::{Formant, Frequency};

/// How many frames old the returned F1–F4 are (the back half of the median window). 20 ms when analysing every 10 ms.
pub const FORMANT_DELAY_FRAMES: usize = 2;
const MEDIAN_FRAMES: usize = 2 * FORMANT_DELAY_FRAMES + 1;
/// Reference frequencies for the assignment (the values in the description of Praat's Formant: Track...). These are for the default
/// ceiling (5500 Hz) and are scaled with the ceiling. The ceiling is chosen to match the resonances of the voice, and the shifted voice
/// is analysed with the ceiling raised by the formant factor. With fixed values, a voice with formant ×0.8 put F3 into the F2 slot.
const REFERENCES: [f64; 4] = [550.0, 1650.0, 2750.0, 3850.0];
const REFERENCE_CEILING: f64 = 5500.0;
/// Cost of an empty slot (used only when there are fewer than 4 candidates).
const EMPTY_COST: f64 = 3.0;

/// An assignment of candidate indices to F1–F4. `None` is an empty slot.
type Assignment = [Option<usize>; 4];
/// The candidates of one frame and the accumulated cost of each assignment.
type Frame = (Vec<Formant>, Vec<(Assignment, f64)>);

#[derive(Default)]
pub struct FormantTracker {
    /// The previous frame's candidates and the accumulated cost of each assignment
    previous: Option<Frame>,
    /// Recent assignment results (newest last)
    recent: VecDeque<[Option<Formant>; 4]>,
}

impl FormantTracker {
    /// Takes the candidates of a voiced frame (in frequency order) and the analysis ceiling (Hz), and returns F1–F4 of the frame `FORMANT_DELAY_FRAMES` earlier.
    /// At the start of voicing, before there are enough frames, it uses the frames it has.
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
        // Subtract the minimum so the accumulated cost does not keep growing
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

    /// Called for an unvoiced frame. Breaks the continuity.
    pub fn reset(&mut self) {
        self.previous = None;
        self.recent.clear();
    }

    /// The median over `FORMANT_DELAY_FRAMES` frames on each side (as far as available) of the frame `FORMANT_DELAY_FRAMES + 1` from the end.
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

/// Lists every assignment from `n` candidates to F1–F4 that keeps the frequency order.
/// With 4 or more candidates, only those that fill all 4 slots; with fewer than 4, empty slots are allowed.
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

/// Combinations of k items chosen from 0..n (in ascending order).
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

    /// When a wide-bandwidth root gets in between F2 and F3 for a single frame, F3 and F4 stay on the real resonances
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

    /// The assignment alone (before the median) does not shift the numbering in the frame with the false root
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

    /// A single-frame outlier disappears in the median, and values come out FORMANT_DELAY_FRAMES late
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
        // The change to 1800 is at frame index 7. The centred median keeps the step in place and it comes out 2 frames late, at index 9
        assert_eq!(outs[8], 1200.0, "{outs:?}");
        assert_eq!(outs[9], 1800.0, "{outs:?}");
    }

    /// Resetting on an unvoiced frame does not carry over values from the previous voice
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

    /// For a voice analysed with a lower ceiling (formant ×0.8), the references go down too, and the lowered F2 goes into the F2 slot
    #[test]
    fn references_follow_the_ceiling() {
        let mut t = FormantTracker::default();
        let c = vec![fm(560.0, 80.0), fm(960.0, 90.0), fm(2080.0, 120.0), fm(2800.0, 150.0), fm(3600.0, 200.0)];
        assert_eq!(hz(t.push(&c, 4400.0)), [Some(560.0), Some(960.0), Some(2080.0), Some(2800.0)]);
    }

    /// With fewer than 4 candidates, they go into the slots with the nearest references and the rest are empty
    #[test]
    fn fewer_candidates_leave_empty_slots() {
        let mut t = FormantTracker::default();
        let c = vec![fm(700.0, 80.0), fm(1200.0, 90.0), fm(2600.0, 120.0)];
        assert_eq!(hz(t.push(&c, 5500.0)), [Some(700.0), Some(1200.0), Some(2600.0), None]);
    }
}
