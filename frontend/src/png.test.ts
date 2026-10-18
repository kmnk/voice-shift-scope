import { describe, expect, it } from "vitest";
import { fileName, layoutLower } from "./png";

describe("fileName", () => {
  it("日時の入った名前にする", () => {
    expect(fileName(new Date(2026, 9, 18, 9, 5, 7))).toBe("voice-shift-scope-20261018-090507.png");
  });
});

describe("layoutLower", () => {
  it("スペクトルと数値の表が重ならず、画像の幅に収まる", () => {
    for (const [total, table, aspect] of [
      [1200, 380, 3.5],
      [900, 420, 2.0],
      [600, 400, 3.0],
    ]) {
      const l = layoutLower(total, table, aspect);
      expect(l.spectrumX + l.spectrumWidth).toBeLessThanOrEqual(l.tableX);
      expect(l.tableX + table).toBeLessThanOrEqual(l.width);
      expect(l.spectrumHeight).toBeCloseTo(l.spectrumWidth / aspect);
    }
  });
});
