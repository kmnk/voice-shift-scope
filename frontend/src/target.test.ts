import { describe, expect, it } from "vitest";
import { targetLineYs } from "./target";

describe("重ね描きの破線の高さ", () => {
  it("周波数の高さに引き、値の無いフォルマントは引かない", () => {
    // 5500 Hz を 550px に描くので 1px = 10 Hz
    expect(targetLineYs([754, null, 2550, NaN], 5500, 550)).toEqual([550 - 75.4, null, 550 - 255, null]);
  });

  it("縦の範囲が決まっていないときは引かない", () => {
    expect(targetLineYs([754], 0, 550)).toEqual([null]);
  });
});
