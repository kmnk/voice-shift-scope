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

describe("marks on the spectrum plot", () => {
  // Plot 700×345px, scale factor 1
  const l = spectrumLayout(700, 345, 1);

  it("takes a 34px dB scale on the left and a 56px margin on the right, and lowers the plot by the mark band", () => {
    expect(l.plotX).toBe(34);
    expect(l.plotW).toBe(700 - 34 - 56);
    expect(l.plotH).toBe(345 - 16 - 22);
    expect(l.rowH).toBe(8);
    // 2px between the upper and lower rows
    expect(l.rowTop[1] - (l.rowTop[0] + l.rowH)).toBe(2);
  });

  it("puts the marks below the curve area, clear of the scale text (S1)", () => {
    expect(l.rowTop[0]).toBeGreaterThanOrEqual(l.plotH);
    // The scale text is 10px high, based 4px from the bottom
    expect(l.rowTop[1] + l.rowH).toBeLessThanOrEqual(345 - 4 - 10);
  });

  it("puts the original F1–F4 in the upper row and the shifted F1–F4 in the lower row at their frequencies (S2, S4)", () => {
    const before = formantMarks(frame([760, 1210, 2550, 3425]), "before", 5500, l);
    const after = formantMarks(frame([795, 1250, 2560, 3345]), "after", 5500, l);
    expect(before.map((m) => m.y)).toEqual([l.rowTop[0], l.rowTop[0], l.rowTop[0], l.rowTop[0]]);
    expect(after.every((m) => m.y === l.rowTop[1])).toBe(true);
    // The mark centre is at the same formula as the 1k scale (left edge + frequency / ceiling × width)
    before.forEach((m, i) => expect(m.x + m.w / 2).toBeCloseTo(l.plotX + ([760, 1210, 2550, 3425][i] / 5500) * l.plotW));
    // Colours are read from the F1–F4 CSS variables
    expect(before.map((m) => m.color)).toEqual(FORMANT_TOKENS.map((n) => `var(--${n})`));
    expect(before.every((m) => m.w === 3 && m.h === 8)).toBe(true);
  });

  it("shows no marks for frames without voice or for a missing side", () => {
    expect(formantMarks(frame([760, 1210, 2550, 3425], false), "before", 5500, l)).toEqual([]);
    expect(formantMarks(null, "after", 5500, l)).toEqual([]);
    expect(formantMarks(frame([760, null, 2550, null]), "after", 5500, l)).toHaveLength(2);
  });

  it("applies the scale factor", () => {
    const l2 = spectrumLayout(1400, 690, 2);
    expect(l2.plotX).toBe(68);
    expect(l2.plotW).toBe(1400 - 68 - 112);
    expect(l2.rowH).toBe(16);
  });
});

describe("dB scale of the spectrum (S6)", () => {
  it("every 20 dB from 0 to −100", () => {
    expect(dbTicks()).toEqual([0, -20, -40, -60, -80, -100]);
  });

  it("writes negative numbers with the minus sign", () => {
    expect(dbTicks().map(dbLabel)).toEqual(["0", "−20", "−40", "−60", "−80", "−100"]);
  });
});

describe("values at the right edge of a spectrogram (S4)", () => {
  it("places them at the frequency height and writes whole Hz", () => {
    const labels = valueLabels([761, 1214, 2552, 3426], 5500, 550, 12);
    const byIndex = Object.fromEntries(labels.map((l) => [l.index, l]));
    expect(byIndex[0].text).toBe("761");
    expect(byIndex[0].lineY).toBeCloseTo(550 - 76.1);
    expect(byIndex[3].y).toBeCloseTo(550 - 342.6);
  });

  it("pushes the lower of two values closer than 12px down so they do not overlap", () => {
    // 5500 Hz is drawn over 550px, so 1px = 10 Hz. 1000 and 1050 are only 5px apart
    const labels = valueLabels([1000, 1050, null, null], 5500, 550, 12);
    const ys = labels.map((l) => l.y).sort((a, b) => a - b);
    expect(ys[1] - ys[0]).toBeGreaterThanOrEqual(12);
    // F2, above (higher frequency), does not move
    const f2 = labels.find((l) => l.index === 1)!;
    expect(f2.y).toBeCloseTo(f2.lineY);
  });

  it("does not show formants without a value", () => {
    expect(valueLabels([null, 1200, null, NaN], 5500, 550, 12).map((l) => l.index)).toEqual([1]);
  });

  it("places the held side's target values too, with none of the 8 closer than 12px", () => {
    // The current and target values differ by only 2 to 4 Hz (less than 1px)
    const labels = valueLabels([738, 1190, 2532, 3455], 5500, 550, 12, [754, 1207, 2550, 3493]);
    expect(labels).toHaveLength(8);
    expect(labels.filter((l) => l.target).map((l) => l.text).sort()).toEqual(["1207", "2550", "3493", "754"]);
    const ys = labels.map((l) => l.y).sort((a, b) => a - b);
    for (let i = 1; i < ys.length; i++) expect(ys[i] - ys[i - 1]).toBeGreaterThanOrEqual(12 - 1e-9);
    // The target is at a higher frequency, so its text comes above the current value
    for (let i = 0; i < 4; i++) {
      const goal = labels.find((l) => l.target && l.index === i)!;
      const now = labels.find((l) => !l.target && l.index === i)!;
      expect(goal.y).toBeLessThan(now.y);
    }
  });

  it("shows only the current values when there are no targets", () => {
    expect(valueLabels([761, null, null, null], 5500, 550, 12).every((l) => !l.target)).toBe(true);
  });

  it("moves values beyond the bottom of the plot to the bottom", () => {
    const labels = valueLabels([10, 20, 30, null], 5500, 550, 12);
    expect(Math.max(...labels.map((l) => l.y))).toBeLessThanOrEqual(550 - 6);
  });
});

describe("cursor values (S7)", () => {
  it("frequency from the height of a spectrogram", () => {
    expect(gramFreqAt(0, 500, 5500)).toBe(5500);
    expect(gramFreqAt(500, 500, 5500)).toBe(0);
    expect(gramFreqAt(250, 500, 5500)).toBe(2750);
  });

  it("frequency and dB from a position on the spectrum, null outside the curve", () => {
    const l = spectrumLayout(700, 345, 1);
    expect(spectrumFreqAt(l.plotX + l.plotW / 2, l, 5500)).toBeCloseTo(2750);
    expect(spectrumFreqAt(l.plotX - 1, l, 5500)).toBeNull();
    expect(spectrumDbAt(0, l)).toBeCloseTo(0);
    expect(spectrumDbAt(l.plotH, l)).toBeCloseTo(-100);
    expect(spectrumDbAt(l.plotH + 1, l)).toBeNull();
  });

  it("label format", () => {
    expect(formatKHz(2031.4)).toBe("2.03 kHz");
    expect(formatDb(-37.6)).toBe("−38 dB");
    expect(formatDb(-0.2)).toBe("0 dB");
  });
});

describe("no hard-coded colours (S1)", () => {
  it("canvas drawing parts read colours from CSS variables", () => {
    for (const [file, src] of [["render.ts", renderSrc], ["png.ts", pngSrc], ["cursor.ts", cursorSrc]]) {
      // So that nothing passes when the value cannot be read (empty)
      expect(src.length, file).toBeGreaterThan(1000);
      expect(src.match(/#[0-9a-fA-F]{3,8}\b/g), file).toBeNull();
    }
  });

  it("has every S1 variable in :root of style.css", () => {
    const root = css.slice(css.indexOf(":root"), css.indexOf("}", css.indexOf(":root")));
    const names = [
      "bg", "surface", "control", "control-hover", "ink", "muted", "line", "grid", "accent", "hold", "danger", "harmonic", "before",
      "formant-1", "formant-2", "formant-3", "formant-4", "space-1", "space-2", "space-3", "radius-s", "radius-m", "text-s", "text-m", "text-l",
    ];
    for (const n of names) expect(root, n).toContain(`--${n}:`);
  });
});
