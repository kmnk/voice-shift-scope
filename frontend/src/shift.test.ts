import { describe, expect, it } from "vitest";
import { fillRange, formantToSlider, formatFormant, formatPitch, normalize, parseShift, sliderToFormant } from "./shift";

describe("ピッチとフォルマントの書式", () => {
  it("ピッチは符号つきで小数 1 桁、負はマイナス記号、0 は符号なし", () => {
    expect(formatPitch(5)).toBe("+5.0");
    expect(formatPitch(0)).toBe("0.0");
    expect(formatPitch(-3.2)).toBe("−3.2");
    expect(formatPitch(-0.04)).toBe("0.0");
  });

  it("フォルマントは小数 2 桁", () => {
    expect(formatFormant(1)).toBe("1.00");
    expect(formatFormant(1.049999)).toBe("1.05");
  });

  it("範囲で止め、刻みを足し続けても誤差が残らない", () => {
    expect(normalize("pitch", 30)).toBe(24);
    expect(normalize("formant", 0.1)).toBe(0.5);
    let v = 0;
    for (let i = 0; i < 11; i++) v = normalize("pitch", v + 0.1);
    expect(v).toBe(1.1);
  });
});

describe("欄の文字の読み取り", () => {
  it("+・-・− を符号として受ける", () => {
    expect(parseShift("+5")).toBe(5);
    expect(parseShift("−3")).toBe(-3);
    expect(parseShift("-0.5")).toBe(-0.5);
    expect(parseShift(" 1.25 ")).toBe(1.25);
  });

  it("数として読めない文字は null", () => {
    expect(parseShift("abc")).toBeNull();
    expect(parseShift("")).toBeNull();
    expect(parseShift("1..2")).toBeNull();
  });
});

describe("フォルマントの対数のスライダー", () => {
  it("×1 が中央、×0.5 と ×2 が両端", () => {
    expect(sliderToFormant(0)).toBe(1);
    expect(sliderToFormant(-1)).toBe(0.5);
    expect(sliderToFormant(1)).toBe(2);
    expect(formantToSlider(1)).toBe(0);
  });

  it("倍率 → 位置 → 倍率で値が変わらない", () => {
    for (let r = 0.5; r <= 2; r = normalize("formant", r + 0.01)) {
      expect(sliderToFormant(formantToSlider(r))).toBe(r);
      if (r === 2) break;
    }
  });
});

describe("スライダーの埋まった部分", () => {
  it("中央からつまみまで", () => {
    expect(fillRange(-24, 24, 0)).toEqual([50, 50]);
    expect(fillRange(-24, 24, 12)).toEqual([50, 75]);
    expect(fillRange(-24, 24, -12)).toEqual([25, 50]);
  });
});
