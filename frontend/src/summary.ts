// 数値の表に出す値。フレームごとの値はぶれるので、直近の中央値を出す。

import type { Frame } from "./types";

export interface Summary {
  f0: number | null;
  formants: ({ frequency: number; bandwidth: number } | null)[];
  harmonics: ({ frequency: number; level: number; observed: boolean } | null)[];
  observedHarmonics: number | null;
  /// 有声のフレームが無く、直前の値を残しているとき true
  stale: boolean;
}

function median(values: number[]): number | null {
  if (values.length === 0) return null;
  const v = [...values].sort((a, b) => a - b);
  return v[Math.floor(v.length / 2)];
}

/// 直近のフレーム（目安 250 ms 分）から、有声のフレームの中央値を出す。
/// 有声のフレームが無ければ `previous` を薄く残す（stale）。
export function summarize(frames: Frame[], previous: Summary | null): Summary | null {
  const voiced = frames.filter((f) => f.voiced && f.f0 !== null);
  if (voiced.length === 0) return previous ? { ...previous, stale: true } : null;
  const formants = [0, 1, 2, 3].map((i) => {
    const f = median(voiced.flatMap((v) => (v.formants[i] ? [v.formants[i]!.frequency] : [])));
    const b = median(voiced.flatMap((v) => (v.formants[i] ? [v.formants[i]!.bandwidth] : [])));
    return f === null || b === null ? null : { frequency: f, bandwidth: b };
  });
  const harmonics = Array.from({ length: 10 }, (_, k) => {
    const hs = voiced.flatMap((v) => v.harmonics.filter((h) => h.number === k + 1));
    const f = median(hs.map((h) => h.frequency));
    const l = median(hs.map((h) => h.level));
    if (f === null || l === null) return null;
    return { frequency: f, level: l, observed: hs.filter((h) => h.observed).length * 2 > hs.length };
  });
  return {
    f0: median(voiced.map((v) => v.f0!)),
    formants,
    harmonics,
    observedHarmonics: median(voiced.map((v) => v.observedHarmonics)),
    stale: false,
  };
}
