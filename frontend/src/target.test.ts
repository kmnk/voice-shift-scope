import { describe, expect, it } from "vitest";
import { targetLineYs } from "./target";

describe("height of the overlay dashed lines", () => {
  it("draws at the frequency heights and skips formants without a value", () => {
    // 5500 Hz is drawn over 550px, so 1px = 10 Hz
    expect(targetLineYs([754, null, 2550, NaN], 5500, 550)).toEqual([550 - 75.4, null, 550 - 255, null]);
  });

  it("draws nothing while the vertical range is not set", () => {
    expect(targetLineYs([754], 0, 550)).toEqual([null]);
  });
});
