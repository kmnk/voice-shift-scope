# /// script
# requires-python = ">=3.11"
# dependencies = ["praat-parselmouth==0.4.7", "numpy>=2", "scipy>=1.11", "soundfile>=0.12"]
# ///
"""Q1（ピッチを下げたときの周期性）を判定する。

揺れのある合成の母音（あいうえお、各 0.7 秒。vibrato 5.5 Hz・±30 cent、ゆっくりした揺れ、
jitter 0.5%・shimmer 3%、−30 dB の息の雑音、F1〜F5 の共鳴）を F0 110 Hz と 220 Hz で作り、
`voice-cli shift` でピッチ −5・−12 半音、フォルマント 1.0・0.8 倍に変える。
狙いの F0 とフォルマントで作り直した理想の出力と、Praat の HNR（harmonicity cc、10 ms、下限 30 Hz の平均）を比べ、
変えた後が理想より 2.0 dB を超えて低ければ FAIL。

`--speech <wav>...` を渡すと、Q10（ピッチを下げたとき元の高さが混ざらない）も判定する。渡した声（48 kHz、mono）を
−12 半音と、−12 半音・フォルマント 0.8 倍で変え、入力が有声の 10 ms の枠のうち、出力の F0 が入力の F0 の ±8% に
ある枠（元の高さのまま出た枠）が 2% を超えれば FAIL。同じ声で Q11（言葉の頭で今の処理より雑音が増えない）も
判定する。入力が有声になってから 5 枠（50 ms）の時刻での出力の HNR の平均が、変更の前の処理（`--shifter signalsmith`）
の同じ値より 0.5 dB を超えて低ければ FAIL。Q12（下げた声で元の高さが聞こえない）も判定する。入力の有声の続きのうち
0.2 秒以上のものごとに、出力の狙いの F0 の奇数番目の倍音と偶数番目の倍音（偶数番目は元の声の倍音と同じ周波数）の
強さの比（3 kHz まで、10 ms ごと、中央値）を求め、−4 dB より小さい続きがあれば FAIL。Q13（子音がこもらない）も
判定する。入力が無声で −45 dBFS より大きく、スペクトルの重心が 4 kHz より上の 10 ms の枠（さしすせその子音などの
摩擦の音）での出力のスペクトルの重心の中央値が、
入力の同じ値にフォルマントの倍率を掛けたものから ±10% に無ければ FAIL。macOS なら、たとえば次で作れる:

    say -v Kyoko -o kyoko.aiff "あいうえお。今日はいい天気ですね。" && afconvert -f WAVE -d LEF32@48000 -c 1 kyoko.aiff kyoko.wav

FAIL があれば終了コード 1。

    uv run scripts/quality_check.py [--voice-cli target/release/voice-cli] [--shifter app|signalsmith] [--keep <dir>] [--speech <wav>...]

wav は一時ディレクトリに書いて消す。`--keep` を渡すと、そこに残す。先に `cargo build --release -p voice-cli` が要る。
"""

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
import parselmouth
import soundfile as sf
from scipy.signal import lfilter

SR = 48000
VOWELS = {
    "a": [(730, 80), (1090, 90), (2440, 120), (3400, 150), (4300, 200)],
    "i": [(300, 60), (2250, 100), (3000, 150), (3700, 200), (4500, 250)],
    "u": [(330, 60), (1300, 90), (2250, 120), (3300, 150), (4300, 200)],
    "e": [(500, 70), (1800, 100), (2550, 130), (3500, 160), (4400, 200)],
    "o": [(500, 70), (850, 80), (2450, 120), (3350, 150), (4300, 200)],
}
CASES = [(-5.0, 1.0), (-12.0, 1.0), (-5.0, 0.8), (-12.0, 0.8)]
TOLERANCE_DB = 2.0
LEAK_TOLERANCE = 0.02
ONSET_TOLERANCE_DB = 0.5
ONSET_FRAMES = 5
ODD_EVEN_MIN_DB = -4.0
ODD_EVEN_MIN_RUN = 0.2
CONSONANT_MIN_DBFS = -45.0
CONSONANT_TOLERANCE = 0.10
CONSONANT_MIN_CENTROID = 4000.0


def synth_vowels(f0, formant_scale, seed=1):
    """揺れのある合成の母音。同じ seed なら同じ揺れになる。"""
    rng = np.random.default_rng(seed)
    seq, seg = "aiueo", 0.7
    n = int(seg * len(seq) * SR)
    t = np.arange(n) / SR
    cents = 30 * np.sin(2 * np.pi * 5.5 * t) + 50 * np.sin(2 * np.pi * 0.3 * t)
    f0t = f0 * 2 ** (cents / 1200)
    src = np.zeros(n)
    i = 0
    while i < n:
        period = SR / f0t[i] * (1 + rng.normal(0, 0.005))
        src[i] = 1 + rng.normal(0, 0.03)
        i += max(1, int(round(period)))
    src = lfilter([1], [1, -1.9 * 0.97, 0.9409], src)
    src = np.diff(src, prepend=0)
    src /= np.max(np.abs(src))
    src += rng.normal(0, 1, n) * np.std(src) * 10 ** (-30 / 20)
    y = np.zeros(n)
    cross = int(0.08 * SR)
    for k, v in enumerate(seq):
        a = max(0, int(k * seg * SR) - cross)
        b = min(n, int((k + 1) * seg * SR) + cross)
        x = src[a:b]
        for fc, bw in VOWELS[v]:
            fc, bw = fc * formant_scale, bw * formant_scale
            if fc >= SR / 2 - 1000:
                continue
            r = np.exp(-np.pi * bw / SR)
            th = 2 * np.pi * fc / SR
            a1, a2 = 2 * r * np.cos(th), -r * r
            x = lfilter([1 - a1 - a2], [1, -a1, -a2], x)
        w = np.ones(b - a)
        ramp = np.linspace(0, 1, min(2 * cross, b - a))
        if a > 0:
            w[: len(ramp)] = ramp
        if b < n:
            w[-len(ramp):] = ramp[::-1]
        y[a:b] += x * w
    fade = int(0.03 * SR)
    y[:fade] *= np.linspace(0, 1, fade)
    y[-fade:] *= np.linspace(1, 0, fade)
    return (y / np.max(np.abs(y)) * 0.5).astype(np.float32)


def hnr(y):
    h = parselmouth.Sound(y.astype(np.float64), SR).to_harmonicity_cc(0.01, 30).values[0]
    return float(np.mean(h[h > -100]))


def pitch_track(y):
    return parselmouth.Sound(y.astype(np.float64), SR).to_pitch_ac(0.01, 40, 600).selected_array["frequency"]


def original_pitch_fraction(x, y):
    """入力が有声の枠のうち、出力の F0 が入力の F0 の ±8% にある枠の割合。"""
    a, b = pitch_track(x), pitch_track(y)
    n = min(len(a), len(b))
    a, b = a[:n], b[:n]
    v = a > 0
    return float(np.mean((b[v] > 0) & (np.abs(b[v] / a[v] - 1) < 0.08))) if np.any(v) else 0.0


def onset_hnr(x, y):
    """入力が有声になってから ONSET_FRAMES 枠（10 ms ごと）の時刻での、出力の HNR の平均。"""
    pitch = parselmouth.Sound(x.astype(np.float64), SR).to_pitch_ac(0.01, 40, 600)
    f0, times = pitch.selected_array["frequency"], pitch.xs()
    run = np.zeros(len(f0), int)
    for i in range(len(f0)):
        run[i] = (run[i - 1] + 1 if i > 0 else 1) if f0[i] > 0 else 0
    onset = times[(run >= 1) & (run <= ONSET_FRAMES)]
    h = parselmouth.Sound(y.astype(np.float64), SR).to_harmonicity_cc(0.01, 30)
    v = h.values[0][np.clip(np.searchsorted(h.xs(), onset), 0, h.values.shape[1] - 1)]
    return float(np.mean(v[v > -100]))


def odd_even_runs(x, y, ratio):
    """入力の有声の続き（ODD_EVEN_MIN_RUN 秒以上）ごとの、出力の奇数番目と偶数番目の倍音の強さの比（dB、中央値）。"""
    pitch = parselmouth.Sound(x.astype(np.float64), SR).to_pitch_ac(0.01, 40, 600)
    f0, times = pitch.selected_array["frequency"], pitch.xs()
    w = int(0.04 * SR)
    freqs = np.fft.rfftfreq(8192, 1 / SR)
    runs, start = [], None
    for i, v in enumerate(list(f0) + [0.0]):
        if v > 0 and start is None:
            start = i
        elif v <= 0 and start is not None:
            if times[i - 1] - times[start] >= ODD_EVEN_MIN_RUN:
                runs.append((start, i))
            start = None
    out = []
    for a, b in runs:
        values = []
        for t, f in zip(times[a:b], f0[a:b]):
            c = int(t * SR)
            if c - w // 2 < 0 or c + w // 2 > len(y):
                continue
            spec = np.abs(np.fft.rfft(y[c - w // 2:c + w // 2] * np.hanning(w), 8192)) ** 2
            target = f * ratio

            def power(h):
                i = int(np.argmin(np.abs(freqs - h * target)))
                return spec[max(0, i - 2):i + 3].max()

            odd = sum(power(h) for h in range(1, 80, 2) if h * target < 3000)
            even = sum(power(h) for h in range(2, 80, 2) if h * target < 3000)
            values.append(10 * np.log10(odd / even))
        out.append((float(times[a]), float(times[b - 1]), float(np.median(values))))
    return out


def consonant_centroid(x, y):
    """入力が無声で CONSONANT_MIN_DBFS より大きく、重心が CONSONANT_MIN_CENTROID より上の枠での、
    入力と出力のスペクトルの重心（Hz、中央値）。重心の低い無声の枠は、かすれた声など音程を下げるのが正しい所を含む。"""
    pitch = parselmouth.Sound(x.astype(np.float64), SR).to_pitch_ac(0.01, 40, 600)
    w = 960
    freqs = np.fft.rfftfreq(w, 1 / SR)

    def frame(sig, t):
        c = int(t * SR)
        seg = sig[c - w // 2:c + w // 2] * np.hanning(w)
        spec = np.abs(np.fft.rfft(seg)) ** 2
        return float((spec * freqs).sum() / (spec.sum() + 1e-20)), 10 * np.log10(np.mean(seg ** 2) + 1e-20)

    times = [t for t, f in zip(pitch.xs(), pitch.selected_array["frequency"])
             if f == 0 and w // 2 <= int(t * SR) <= min(len(x), len(y)) - w // 2 and frame(x, t)[1] > CONSONANT_MIN_DBFS
             and frame(x, t)[0] > CONSONANT_MIN_CENTROID]
    return float(np.median([frame(x, t)[0] for t in times])), float(np.median([frame(y, t)[0] for t in times])), len(times)


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--voice-cli", default="target/release/voice-cli")
    p.add_argument("--shifter", default="app", choices=["app", "signalsmith"])
    p.add_argument("--keep", type=Path)
    p.add_argument("--speech", type=Path, nargs="*", default=[])
    args = p.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        d = args.keep or Path(tmp)
        d.mkdir(parents=True, exist_ok=True)
        failed = False
        for f0 in (110.0, 220.0):
            src = d / f"vowels_{f0:.0f}.wav"
            sf.write(src, synth_vowels(f0, 1.0), SR, subtype="FLOAT")
            for st, fm in CASES:
                out = d / f"vowels_{f0:.0f}_{st:+.0f}_{fm}.wav"
                subprocess.run(
                    [args.voice_cli, "shift", str(src), str(out), "--pitch", str(st), "--formant", str(fm),
                     "--shifter", args.shifter],
                    check=True,
                )
                y, _ = sf.read(out, dtype="float32")
                ideal = synth_vowels(f0 * 2 ** (st / 12), fm)
                h_out, h_ideal = hnr(y), hnr(ideal)
                ok = h_out >= h_ideal - TOLERANCE_DB
                failed |= not ok
                print(json.dumps({
                    "criterion": "Q1", "f0": f0, "semitones": st, "formant": fm,
                    "hnr_db": round(h_out, 2), "ideal_hnr_db": round(h_ideal, 2),
                    "diff_db": round(h_out - h_ideal, 2), "tolerance_db": TOLERANCE_DB,
                    "pass": ok,
                }, ensure_ascii=False))
        for wav in args.speech:
            x, rate = sf.read(wav, dtype="float32")
            if rate != SR:
                sys.exit(f"{wav} は {rate} Hz です。{SR} Hz の wav を渡してください")
            for st, fm in [(-12.0, 1.0), (-12.0, 0.8)]:
                out = d / f"{wav.stem}_{st:+.0f}_{fm}.wav"
                subprocess.run(
                    [args.voice_cli, "shift", str(wav), str(out), "--pitch", str(st), "--formant", str(fm),
                     "--shifter", args.shifter],
                    check=True,
                )
                y, _ = sf.read(out, dtype="float32")
                base = d / f"{wav.stem}_{st:+.0f}_{fm}_signalsmith.wav"
                subprocess.run(
                    [args.voice_cli, "shift", str(wav), str(base), "--pitch", str(st), "--formant", str(fm),
                     "--shifter", "signalsmith"],
                    check=True,
                )
                yb, _ = sf.read(base, dtype="float32")
                h_on, h_base = onset_hnr(x, y), onset_hnr(x, yb)
                ok11 = h_on >= h_base - ONSET_TOLERANCE_DB
                failed |= not ok11
                print(json.dumps({
                    "criterion": "Q11", "input": wav.name, "semitones": st, "formant": fm,
                    "onset_hnr_db": round(h_on, 2), "before_change_onset_hnr_db": round(h_base, 2),
                    "diff_db": round(h_on - h_base, 2), "tolerance_db": ONSET_TOLERANCE_DB, "pass": ok11,
                }, ensure_ascii=False))
                runs = odd_even_runs(x, y, 2 ** (st / 12))
                worst = min(runs, key=lambda r: r[2])
                ok12 = worst[2] >= ODD_EVEN_MIN_DB
                failed |= not ok12
                print(json.dumps({
                    "criterion": "Q12", "input": wav.name, "semitones": st, "formant": fm,
                    "worst_run_seconds": [round(worst[0], 2), round(worst[1], 2)],
                    "worst_odd_even_db": round(worst[2], 2), "min_db": ODD_EVEN_MIN_DB, "pass": ok12,
                }, ensure_ascii=False))
                c_in, c_out, frames = consonant_centroid(x, y)
                expected = c_in * fm
                ok13 = abs(c_out / expected - 1) <= CONSONANT_TOLERANCE
                failed |= not ok13
                print(json.dumps({
                    "criterion": "Q13", "input": wav.name, "semitones": st, "formant": fm, "frames": frames,
                    "input_centroid_hz": round(c_in), "expected_hz": round(expected), "output_centroid_hz": round(c_out),
                    "diff_percent": round((c_out / expected - 1) * 100, 1), "tolerance_percent": CONSONANT_TOLERANCE * 100,
                    "pass": ok13,
                }, ensure_ascii=False))
                leak = original_pitch_fraction(x, y)
                ok = leak <= LEAK_TOLERANCE
                failed |= not ok
                print(json.dumps({
                    "criterion": "Q10", "input": wav.name, "semitones": st, "formant": fm,
                    "original_pitch_fraction": round(leak, 4), "tolerance": LEAK_TOLERANCE, "pass": ok,
                }, ensure_ascii=False))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
