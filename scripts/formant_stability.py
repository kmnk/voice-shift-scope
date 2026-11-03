# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=2", "scipy>=1.11"]
# ///
"""Checks the stability of the F1–F4 dots (the values in the scrolling plots): B9–B12 in docs/criteria.md.

Makes vowels with variation (jitter, shimmer) and breath noise, and vowels whose F2 moves, and compares the values from `voice-cli analyze` (the same analysis parts as the app)
with those from `voice-cli analyze --raw-formants` (no tracking; the 4 lowest roots of each frame).
With `--speech <wav>[:<ceiling Hz>]` and a voice wav (48 kHz, mono), also checks on the voice. On macOS, a voice can be made with:

    say -v Kyoko -o kyoko.aiff "あいうえお。今日はいい天気ですね。わたしは、声の研究をしています。"
    afconvert -f WAVE -d LEF32@48000 -c 1 kyoko.aiff kyoko.wav

Checks (exit code 1 if any FAIL):
- B9 For steady vowels (F0 120, 183, and 250 Hz; HNR 20 and 10 dB), F2–F4 are within 3% of the truth, F1 is no worse than untracked, and no voiced frame lacks a value
- B10 For moving vowels (F2 moves between 1000 and 2000 Hz in 150 ms and 40 ms), F2's difference from the truth is no worse than untracked
- B11 On a voice, values that stand out from both neighbours in a single frame (spikes) are 0.5% or less for each of F1–F4
- B12 On a voice, the share of frames differing from the next by more than 15% (jumps) does not rise by more than 1 point above untracked

Does not open devices; only makes and reads wavs. It writes only to `--keep <dir>` or a temporary directory, which is deleted at the end.
Needs `cargo build --release -p voice-cli` first.

    uv run scripts/formant_stability.py [--voice-cli target/release/voice-cli] [--keep <dir>] [--json] [--speech <wav>[:<ceiling>]...]
"""

import argparse
import csv
import json
import math
import pathlib
import subprocess
import sys
import tempfile

import numpy as np
from scipy.io import wavfile
from scipy.signal import lfilter

FS = 48000
# The F1–F4 in the scrolling plots are from 2 frames earlier, because of the median after tracking (FORMANT_DELAY_FRAMES in voice-dsp)
DELAY = 2
SKIP = 0.1  # seconds at the start left out of the checks
TRUTH = [700.0, 1200.0, 2600.0, 3500.0]
FORMANTS = [700.0, 1200.0, 2600.0, 3500.0, 4500.0]
BANDWIDTHS = [80.0, 90.0, 120.0, 150.0, 200.0]


# ---- test signals ----

def _resonator(x, f, bw):
    r = math.exp(-math.pi * bw / FS)
    a1 = -2 * r * math.cos(2 * math.pi * f / FS)
    a2 = r * r
    return lfilter([1 + a1 + a2], [1, a1, a2], x)


def _resonator_tv(x, f, bw):
    y = np.zeros_like(x)
    y1 = y2 = 0.0
    r = math.exp(-math.pi * bw / FS)
    for i in range(len(x)):
        a1 = -2 * r * math.cos(2 * math.pi * f[i] / FS)
        a2 = r * r
        v = (1 + a1 + a2) * x[i] - a1 * y1 - a2 * y2
        y2, y1 = y1, v
        y[i] = v
    return y


def glide_f2(t, ramp, lo=1000.0, hi=2000.0):
    """F2 that moves lo→hi at 0.25 s and hi→lo at 0.75 s of each second, over `ramp` seconds."""
    ph = np.asarray(t) % 1.0
    up = np.clip((ph - 0.25) / ramp, 0, 1) - np.clip((ph - 0.75) / ramp, 0, 1)
    return lo + (hi - lo) * up


def vowel(f0, hnr_db, ramp=None, seconds=3.0, seed=1):
    """Rosenberg pulses with jitter 1%, shimmer 5%, a 0.7 Hz 2% F0 drift, and breath noise, passed through the resonances."""
    rng = np.random.default_rng(seed)
    n = int(seconds * FS)
    src = np.zeros(n)
    t = 0.0
    while t < n - 1:
        cur = f0 * (1 + 0.02 * math.sin(2 * math.pi * 0.7 * t / FS)) * (1 + 0.01 * rng.standard_normal())
        period = FS / cur
        amp = 1 + 0.05 * rng.standard_normal()
        op = int(period * 0.6)
        tp = int(op * 0.67)
        k = np.arange(op)
        pulse = np.where(k < tp, 0.5 * (1 - np.cos(np.pi * k / tp)), np.cos(np.pi * (k - tp) / (2 * (op - tp))))
        i0 = int(t)
        seg = min(op, n - i0)
        src[i0:i0 + seg] += amp * pulse[:seg]
        t += period
    src = np.diff(src, prepend=0.0)
    noise = rng.standard_normal(n)
    noise *= np.sqrt(np.mean(src ** 2)) / np.sqrt(np.mean(noise ** 2)) * 10 ** (-hnr_db / 20)
    x = src + noise
    for i, (f, bw) in enumerate(zip(FORMANTS, BANDWIDTHS)):
        if i == 1 and ramp is not None:
            x = _resonator_tv(x, glide_f2(np.arange(n) / FS, ramp), bw)
        else:
            x = _resonator(x, f, bw)
    return (x / np.max(np.abs(x)) * 0.5).astype(np.float32)


# ---- analysis ----

def analyze(voice_cli, wav, ceiling, raw):
    cmd = [voice_cli, "analyze", str(wav), "--ceiling", str(ceiling)]
    if raw:
        cmd.append("--raw-formants")
    out = subprocess.run(cmd, check=True, capture_output=True, text=True).stdout
    rows = list(csv.DictReader(out.splitlines()))
    times = [float(r["time"]) for r in rows]
    voiced = [r["voiced"] == "1" and float(r["time"]) >= SKIP for r in rows]
    track = [[float(r[f"f{i}"]) if r[f"f{i}"] else None for i in range(1, 5)] for r in rows]
    return times, voiced, track


def aligned(times, voiced, track, delay):
    """Aligns to frame times, taking the value in row r as belonging to frame r - delay."""
    n = len(track) - delay
    return (
        times[:n],
        [voiced[t] and voiced[t + delay] for t in range(n)],
        [track[t + delay] for t in range(n)],
    )


# ---- measures ----

def spike_rate(track, voiced, i, th=0.15, flat=0.07):
    n = s = 0
    for t in range(1, len(track) - 1):
        if not (voiced[t - 1] and voiced[t] and voiced[t + 1]):
            continue
        a, b, c = track[t - 1][i], track[t][i], track[t + 1][i]
        if None in (a, b, c):
            continue
        n += 1
        if abs(math.log(a / c)) < flat and abs(math.log(b / a)) > th and abs(math.log(b / c)) > th:
            s += 1
    return 100 * s / n if n else 0.0


def jump_rate(track, voiced, i, th=0.15):
    n = s = 0
    for t in range(1, len(track)):
        if not (voiced[t - 1] and voiced[t]):
            continue
        a, b = track[t - 1][i], track[t][i]
        if None in (a, b):
            continue
        n += 1
        s += abs(math.log(b / a)) > th
    return 100 * s / n if n else 0.0


def rms_err(track, voiced, i, truth):
    e = [math.log(track[t][i] / truth[t][i]) for t in range(len(track)) if voiced[t] and track[t][i] is not None]
    missing = sum(1 for t in range(len(track)) if voiced[t] and track[t][i] is None)
    return (100 * math.sqrt(sum(v * v for v in e) / len(e)) if e else float("inf")), missing


# ---- checks ----

def judge(voice_cli, workdir, speech):
    results = []

    def add(name, target, value, limit, ok):
        results.append({"criterion": name, "target": target, "value": round(value, 2), "limit": round(limit, 2), "pass": ok})

    for f0 in (120, 183, 250):
        for hnr in (20, 10):
            wav = workdir / f"steady-{f0}-{hnr}.wav"
            wavfile.write(wav, FS, vowel(f0, hnr))
            t, v, tr = aligned(*analyze(voice_cli, wav, 5500, False), DELAY)
            rt, rv, rtr = analyze(voice_cli, wav, 5500, True)
            for i in range(4):
                err, miss = rms_err(tr, v, i, [TRUTH] * len(tr))
                label = f"F0 {f0} Hz, HNR {hnr} dB, F{i + 1}"
                if i == 0:
                    raw, _ = rms_err(rtr, rv, i, [TRUTH] * len(rtr))
                    add("B9", label, err, raw, err <= raw + 1e-9)
                else:
                    add("B9", label, err, 3.0, err <= 3.0)
                add("B9", label + ", frames without a value", miss, 0, miss == 0)
    for ramp in (0.15, 0.04):
        wav = workdir / f"glide-{int(ramp * 1000)}ms.wav"
        wavfile.write(wav, FS, vowel(183, 20, ramp=ramp))
        t, v, tr = aligned(*analyze(voice_cli, wav, 5500, False), DELAY)
        rt, rv, rtr = analyze(voice_cli, wav, 5500, True)
        truth = [[700.0, float(glide_f2(x, ramp)), 2600.0, 3500.0] for x in t]
        rtruth = [[700.0, float(glide_f2(x, ramp)), 2600.0, 3500.0] for x in rt]
        err, _ = rms_err(tr, v, 1, truth)
        raw, _ = rms_err(rtr, rv, 1, rtruth)
        add("B10", f"F2 moving in {int(ramp * 1000)} ms, F2", err, raw, err <= raw + 1e-9)
    for spec in speech:
        path, _, ceil = spec.partition(":")
        ceiling = float(ceil) if ceil else 5500.0
        name = f"{pathlib.Path(path).name}, ceiling {ceiling:.0f} Hz"
        t, v, tr = aligned(*analyze(voice_cli, path, ceiling, False), DELAY)
        rt, rv, rtr = analyze(voice_cli, path, ceiling, True)
        for i in range(4):
            s = spike_rate(tr, v, i)
            add("B11", f"{name}, F{i + 1} spikes %", s, 0.5, s <= 0.5)
            j, rj = jump_rate(tr, v, i), jump_rate(rtr, rv, i)
            add("B12", f"{name}, F{i + 1} jumps %", j, rj + 1.0, j <= rj + 1.0)
    return results


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--voice-cli", default="target/release/voice-cli")
    p.add_argument("--keep", help="where to keep the generated wavs (otherwise they go to a temporary directory and are deleted)")
    p.add_argument("--speech", nargs="*", default=[], help="voice wavs (48 kHz, mono). Append `:<ceiling Hz>` to change the ceiling")
    p.add_argument("--json", action="store_true")
    a = p.parse_args()
    if not pathlib.Path(a.voice_cli).exists():
        sys.exit(f"{a.voice_cli} not found. Run `cargo build --release -p voice-cli` first")
    for spec in a.speech:
        if not pathlib.Path(spec.partition(":")[0]).exists():
            sys.exit(f"wav {spec} not found")
    if a.keep:
        workdir = pathlib.Path(a.keep)
        workdir.mkdir(parents=True, exist_ok=True)
        results = judge(a.voice_cli, workdir, a.speech)
    else:
        with tempfile.TemporaryDirectory() as d:
            results = judge(a.voice_cli, pathlib.Path(d), a.speech)
    if a.json:
        print(json.dumps(results, ensure_ascii=False, indent=1))
    else:
        for r in results:
            mark = "PASS" if r["pass"] else "FAIL"
            print(f"{mark} {r['criterion']} {r['target']}: {r['value']} (limit {r['limit']})")
    failed = sum(not r["pass"] for r in results)
    print(f"{len(results) - failed} PASS / {failed} FAIL", file=sys.stderr)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
