//! 受け入れの基準に使う、答えの分かる試験用の信号を作る。
//!
//! 合成の母音: F0 の倍音を同じ強さで並べた帯域制限のパルス列を、
//! F1〜F4 の 2 次の共鳴のフィルタに順に通す。
//! 倍音だけの信号: H1〜Hn を同じ強さで並べ、白色雑音を足す。

use std::f64::consts::PI;

/// 2 次の共鳴（極が 1 組の all-pole）のフィルタ。
#[derive(Debug, Clone, Copy)]
pub struct Resonance {
    pub frequency: f64,
    pub bandwidth: f64,
}

/// spec の合成の母音の F1〜F4（帯域幅は 60〜120 Hz の幅に置く）。
pub const SPEC_VOWEL: [Resonance; 4] = [
    Resonance { frequency: 700.0, bandwidth: 60.0 },
    Resonance { frequency: 1200.0, bandwidth: 80.0 },
    Resonance { frequency: 2600.0, bandwidth: 100.0 },
    Resonance { frequency: 3500.0, bandwidth: 120.0 },
];

/// 合成の母音に使う共鳴。spec の F1〜F4 に、F5（4500 Hz）を足す。
/// 実際の声は上限（5500 Hz）までに 5 つ前後の共鳴を持ち、Praat と同じ既定（5 個・5500 Hz）の LPC は
/// それを前提にしている。F1〜F4 だけの信号では、F2 と F3 の間に無い共鳴を置いてしまう。
pub const SYNTH_VOWEL: [Resonance; 5] = [
    SPEC_VOWEL[0],
    SPEC_VOWEL[1],
    SPEC_VOWEL[2],
    SPEC_VOWEL[3],
    Resonance { frequency: 4500.0, bandwidth: 150.0 },
];

impl Resonance {
    /// 差分方程式 y[n] = g x[n] + a1 y[n-1] + a2 y[n-2] の係数。
    /// g は 0 Hz の利得を 1 にする。
    fn coefficients(&self, sample_rate: f64) -> (f64, f64, f64) {
        let r = (-PI * self.bandwidth / sample_rate).exp();
        let theta = 2.0 * PI * self.frequency / sample_rate;
        let a1 = 2.0 * r * theta.cos();
        let a2 = -r * r;
        let g = 1.0 - a1 - a2;
        (g, a1, a2)
    }

    /// 周波数 `f` での振幅の応答。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn magnitude(&self, f: f64, sample_rate: f64) -> f64 {
        let (g, a1, a2) = self.coefficients(sample_rate);
        let w = 2.0 * PI * f / sample_rate;
        // 1 - a1 e^{-jw} - a2 e^{-2jw}
        let re = 1.0 - a1 * w.cos() - a2 * (2.0 * w).cos();
        let im = a1 * w.sin() + a2 * (2.0 * w).sin();
        g / (re * re + im * im).sqrt()
    }
}

/// 帯域制限のパルス列を共鳴に通した合成の母音。最大の振幅を `peak` にそろえる。
pub fn vowel(f0: f64, resonances: &[Resonance], sample_rate: f64, seconds: f64, peak: f64) -> Vec<f32> {
    let n = (sample_rate * seconds) as usize;
    let harmonics = ((sample_rate / 2.0 - 1.0) / f0).floor() as usize;
    let mut x: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 / sample_rate;
            (1..=harmonics).map(|k| (2.0 * PI * k as f64 * f0 * t).cos()).sum::<f64>()
        })
        .collect();
    for r in resonances {
        let (g, a1, a2) = r.coefficients(sample_rate);
        let (mut y1, mut y2) = (0.0, 0.0);
        for v in x.iter_mut() {
            let y = g * *v + a1 * y1 + a2 * y2;
            y2 = y1;
            y1 = y;
            *v = y;
        }
    }
    normalize(&x, peak)
}

/// H1〜H`count` を同じ強さで並べ、各倍音より `noise_below_db` だけ低い白色雑音を足した信号。
pub fn harmonics(f0: f64, count: usize, noise_below_db: f64, sample_rate: f64, seconds: f64, peak: f64) -> Vec<f32> {
    let n = (sample_rate * seconds) as usize;
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    // 1 本の倍音（振幅 1 の cos）の電力は 1/2。雑音の電力を、窓の 1 bin あたりで比べると
    // 長さに依るので、ここでは全帯域の雑音の標準偏差を倍音の振幅から決める。
    let noise_sd = 10f64.powf(-noise_below_db / 20.0);
    let x: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 / sample_rate;
            let tone: f64 = (1..=count).map(|k| (2.0 * PI * k as f64 * f0 * t).cos()).sum();
            tone + noise_sd * rng.gaussian()
        })
        .collect();
    normalize(&x, peak)
}

fn normalize(x: &[f64], peak: f64) -> Vec<f32> {
    let max = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let scale = if max > 0.0 { peak / max } else { 0.0 };
    x.iter().map(|v| (v * scale) as f32).collect()
}

/// 試験の信号を毎回同じにするための小さな乱数。
struct XorShift(u64);

impl XorShift {
    fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn gaussian(&mut self) -> f64 {
        let u1 = self.next_f64().max(f64::MIN_POSITIVE);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
/// Hann の窓を掛けた、周波数 `f` での振幅（Goertzel と同じ値を直に計算する）。
pub fn amplitude_at(x: &[f32], f: f64, sample_rate: f64) -> f64 {
    let n = x.len();
    let (mut re, mut im, mut wsum) = (0.0, 0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / (n - 1) as f64).cos();
        let ph = 2.0 * PI * f * i as f64 / sample_rate;
        re += w * *v as f64 * ph.cos();
        im -= w * *v as f64 * ph.sin();
        wsum += w;
    }
    2.0 * (re * re + im * im).sqrt() / wsum
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    /// 合成の母音の倍音の強さ（H1 との dB の差）が、共鳴のフィルタの応答から
    /// 計算した値と ±1 dB で合う。F0 は spec の 3 通り。
    #[test]
    fn vowel_harmonics_follow_resonance_response() {
        for f0 in [100.0, 150.0, 220.0] {
            let x = vowel(f0, &SYNTH_VOWEL, SR, 1.5, 0.5);
            let steady = &x[(0.5 * SR) as usize..];
            let response = |f: f64| SYNTH_VOWEL.iter().map(|r| r.magnitude(f, SR)).product::<f64>();
            let measured_h1 = amplitude_at(steady, f0, SR);
            let mut k = 2;
            while (k as f64) * f0 < 4000.0 {
                let f = k as f64 * f0;
                let expected_db = 20.0 * (response(f) / response(f0)).log10();
                let measured_db = 20.0 * (amplitude_at(steady, f, SR) / measured_h1).log10();
                assert!(
                    (measured_db - expected_db).abs() < 1.0,
                    "F0 {f0} H{k}: measured {measured_db:.2} dB, expected {expected_db:.2} dB"
                );
                k += 1;
            }
        }
    }

    /// 倍音だけの信号は、H1〜H8 が雑音の床より 20 dB 以上高く、H9 の位置には山が無い。
    #[test]
    fn harmonics_signal_has_eight_partials_above_noise() {
        let x = harmonics(150.0, 8, 40.0, SR, 1.0, 0.5);
        let floor = amplitude_at(&x, 150.0 * 8.5, SR);
        for k in 1..=8 {
            let a = amplitude_at(&x, 150.0 * k as f64, SR);
            assert!(20.0 * (a / floor).log10() > 20.0, "H{k}");
        }
        let h9 = amplitude_at(&x, 150.0 * 9.0, SR);
        assert!(20.0 * (h9 / floor).log10() < 10.0);
    }
}
