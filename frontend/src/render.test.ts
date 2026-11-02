import { describe, expect, it } from "vitest";
import {
  dbLabel,
  dbTicks,
  formantMarks,
  formatDb,
  formatKHz,
  gramFreqAt,
  spectrumDbAt,
  spectrumFreqAt,
  spectrumLayout,
  valueLabels,
} from "./render";
import { FORMANT_TOKENS } from "./tokens";
import type { Frame } from "./types";
import renderSrc from "./render.ts?raw";
import pngSrc from "./png.ts?raw";
import cursorSrc from "./cursor.ts?raw";
import css from "./style.css?raw";

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

  it("左に 34px の dB の目盛り、右に 56px の余白を取り、図は印の帯の分だけ低くなる", () => {
    expect(l.plotX).toBe(34);
    expect(l.plotW).toBe(700 - 34 - 56);
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
    // 印の中心が、1k ごとの目盛りと同じ式（左端 + 周波数 / 上限 × 幅）の位置
    before.forEach((m, i) => expect(m.x + m.w / 2).toBeCloseTo(l.plotX + ([760, 1210, 2550, 3425][i] / 5500) * l.plotW));
    // 色は F1〜F4 の CSS の変数から読む
    expect(before.map((m) => m.color)).toEqual(FORMANT_TOKENS.map((n) => `var(--${n})`));
    expect(before.every((m) => m.w === 3 && m.h === 8)).toBe(true);
  });

  it("声の出ていないフレームと、無い側では印を出さない", () => {
    expect(formantMarks(frame([760, 1210, 2550, 3425], false), "before", 5500, l)).toEqual([]);
    expect(formantMarks(null, "after", 5500, l)).toEqual([]);
    expect(formantMarks(frame([760, null, 2550, null]), "after", 5500, l)).toHaveLength(2);
  });

  it("拡大率を掛ける", () => {
    const l2 = spectrumLayout(1400, 690, 2);
    expect(l2.plotX).toBe(68);
    expect(l2.plotW).toBe(1400 - 68 - 112);
    expect(l2.rowH).toBe(16);
  });
});

describe("スペクトルの dB の目盛り（S6）", () => {
  it("0 から −100 まで 20 dB ごと", () => {
    expect(dbTicks()).toEqual([0, -20, -40, -60, -80, -100]);
  });

  it("負の数はマイナス記号で書く", () => {
    expect(dbTicks().map(dbLabel)).toEqual(["0", "−20", "−40", "−60", "−80", "−100"]);
  });
});

describe("スペクトログラムの右端の値（S4）", () => {
  it("周波数の高さに置き、整数の Hz で書く", () => {
    const labels = valueLabels([761, 1214, 2552, 3426], 5500, 550, 12);
    const byIndex = Object.fromEntries(labels.map((l) => [l.index, l]));
    expect(byIndex[0].text).toBe("761");
    expect(byIndex[0].lineY).toBeCloseTo(550 - 76.1);
    expect(byIndex[3].y).toBeCloseTo(550 - 342.6);
  });

  it("12px より近い値は、下の値を下へずらして重ねない", () => {
    // 5500 Hz を 550px に描くので 1px = 10 Hz。1000 と 1050 は 5px しか離れない
    const labels = valueLabels([1000, 1050, null, null], 5500, 550, 12);
    const ys = labels.map((l) => l.y).sort((a, b) => a - b);
    expect(ys[1] - ys[0]).toBeGreaterThanOrEqual(12);
    // 上（高い周波数）の F2 は動かさない
    const f2 = labels.find((l) => l.index === 1)!;
    expect(f2.y).toBeCloseTo(f2.lineY);
  });

  it("値の無いフォルマントは出さない", () => {
    expect(valueLabels([null, 1200, null, NaN], 5500, 550, 12).map((l) => l.index)).toEqual([1]);
  });

  it("止めた側の目標の値も一緒に並べ、8 つのどれも 12px より近づかない", () => {
    // 今の値と目標の値が 2〜4 Hz しか違わない（1px 未満）
    const labels = valueLabels([738, 1190, 2532, 3455], 5500, 550, 12, [754, 1207, 2550, 3493]);
    expect(labels).toHaveLength(8);
    expect(labels.filter((l) => l.target).map((l) => l.text).sort()).toEqual(["1207", "2550", "3493", "754"]);
    const ys = labels.map((l) => l.y).sort((a, b) => a - b);
    for (let i = 1; i < ys.length; i++) expect(ys[i] - ys[i - 1]).toBeGreaterThanOrEqual(12 - 1e-9);
    // 目標の方が高い周波数なので、目標の文字が今の値の上に来る
    for (let i = 0; i < 4; i++) {
      const goal = labels.find((l) => l.target && l.index === i)!;
      const now = labels.find((l) => !l.target && l.index === i)!;
      expect(goal.y).toBeLessThan(now.y);
    }
  });

  it("目標が無いときは今の値だけ", () => {
    expect(valueLabels([761, null, null, null], 5500, 550, 12).every((l) => !l.target)).toBe(true);
  });

  it("図の下端を越えるときは下端に寄せる", () => {
    const labels = valueLabels([10, 20, 30, null], 5500, 550, 12);
    expect(Math.max(...labels.map((l) => l.y))).toBeLessThanOrEqual(550 - 6);
  });
});

describe("カーソルの値（S7）", () => {
  it("スペクトログラムの高さから周波数", () => {
    expect(gramFreqAt(0, 500, 5500)).toBe(5500);
    expect(gramFreqAt(500, 500, 5500)).toBe(0);
    expect(gramFreqAt(250, 500, 5500)).toBe(2750);
  });

  it("スペクトルの位置から周波数と dB。曲線の範囲の外は null", () => {
    const l = spectrumLayout(700, 345, 1);
    expect(spectrumFreqAt(l.plotX + l.plotW / 2, l, 5500)).toBeCloseTo(2750);
    expect(spectrumFreqAt(l.plotX - 1, l, 5500)).toBeNull();
    expect(spectrumDbAt(0, l)).toBeCloseTo(0);
    expect(spectrumDbAt(l.plotH, l)).toBeCloseTo(-100);
    expect(spectrumDbAt(l.plotH + 1, l)).toBeNull();
  });

  it("札の書き方", () => {
    expect(formatKHz(2031.4)).toBe("2.03 kHz");
    expect(formatDb(-37.6)).toBe("−38 dB");
    expect(formatDb(-0.2)).toBe("0 dB");
  });
});

describe("色の直書きが無い（S1）", () => {
  it("canvas に描く部品は、色を CSS の変数から読む", () => {
    for (const [file, src] of [["render.ts", renderSrc], ["png.ts", pngSrc], ["cursor.ts", cursorSrc]]) {
      // 読めていない（空）ときに素通りしないように
      expect(src.length, file).toBeGreaterThan(1000);
      expect(src.match(/#[0-9a-fA-F]{3,8}\b/g), file).toBeNull();
    }
  });

  it("style.css の :root に、S1 の変数がすべてある", () => {
    const root = css.slice(css.indexOf(":root"), css.indexOf("}", css.indexOf(":root")));
    const names = [
      "bg", "surface", "control", "control-hover", "ink", "muted", "line", "grid", "accent", "hold", "danger", "harmonic", "before",
      "formant-1", "formant-2", "formant-3", "formant-4", "space-1", "space-2", "space-3", "radius-s", "radius-m", "text-s", "text-m", "text-l",
    ];
    for (const n of names) expect(root, n).toContain(`--${n}:`);
  });
});
