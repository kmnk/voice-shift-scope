import { describe, expect, it } from "vitest";
import { FORMANT_COLORS, formantMarks, spectrumLayout } from "./render";
import type { Frame } from "./types";

const frame = (hz: (number | null)[], voiced = true): Frame => ({
  f0: 150,
  voiced,
  formants: hz.map((f) => (f === null ? null : { frequency: f, bandwidth: 80 })),
  harmonics: [],
  observedHarmonics: 0,
  spectrum: [],
});

describe("スペクトルの図の印", () => {
  // 図 700×345px、拡大率 1
  const l = spectrumLayout(700, 345, 1);

  it("右に 56px の余白を取り、図は印の帯の分だけ低くなる", () => {
    expect(l.plotW).toBe(644);
    expect(l.plotH).toBe(345 - 16 - 22);
    expect(l.rowH).toBe(8);
    // 上の行と下の行の間は 2px
    expect(l.rowTop[1] - (l.rowTop[0] + l.rowH)).toBe(2);
  });

  it("印は曲線を描く範囲の下にあり、目盛りの文字にかからない（S1）", () => {
    expect(l.rowTop[0]).toBeGreaterThanOrEqual(l.plotH);
    // 目盛りの文字は下から 4px を基準に 10px の高さ
    expect(l.rowTop[1] + l.rowH).toBeLessThanOrEqual(345 - 4 - 10);
  });

  it("上の行に変える前、下の行に変えた後の F1〜F4 を、その周波数の位置に置く（S2・S4）", () => {
    const before = formantMarks(frame([760, 1210, 2550, 3425]), "before", 5500, l);
    const after = formantMarks(frame([795, 1250, 2560, 3345]), "after", 5500, l);
    expect(before.map((m) => m.y)).toEqual([l.rowTop[0], l.rowTop[0], l.rowTop[0], l.rowTop[0]]);
    expect(after.every((m) => m.y === l.rowTop[1])).toBe(true);
    // 印の中心が、1k ごとの目盛りと同じ式（周波数 / 上限 × 余白を除いた幅）の位置
    before.forEach((m, i) => expect(m.x + m.w / 2).toBeCloseTo(([760, 1210, 2550, 3425][i] / 5500) * l.plotW));
    expect(before.map((m) => m.color)).toEqual(FORMANT_COLORS);
    expect(before.every((m) => m.w === 3 && m.h === 8)).toBe(true);
  });

  it("声の出ていないフレームと、無い側では印を出さない", () => {
    expect(formantMarks(frame([760, 1210, 2550, 3425], false), "before", 5500, l)).toEqual([]);
    expect(formantMarks(null, "after", 5500, l)).toEqual([]);
    expect(formantMarks(frame([760, null, 2550, null]), "after", 5500, l)).toHaveLength(2);
  });

  it("拡大率を掛ける", () => {
    const l2 = spectrumLayout(1400, 690, 2);
    expect(l2.plotW).toBe(1400 - 112);
    expect(l2.rowH).toBe(16);
  });
});
