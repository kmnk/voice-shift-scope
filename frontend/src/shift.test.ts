import { describe, expect, it } from "vitest";
import { fillRange, formantToSlider, formatFormant, formatPitch, normalize, parseShift, sliderToFormant } from "./shift";

describe("pitch and formant formats", () => {
  it("pitch is signed with 1 decimal place, negatives use the minus sign, and 0 has no sign", () => {
    expect(formatPitch(5)).toBe("+5.0");
    expect(formatPitch(0)).toBe("0.0");
    expect(formatPitch(-3.2)).toBe("−3.2");
    expect(formatPitch(-0.04)).toBe("0.0");
  });

  it("formant has 2 decimal places", () => {
    expect(formatFormant(1)).toBe("1.00");
    expect(formatFormant(1.049999)).toBe("1.05");
  });

  it("clamps to the range and leaves no error after adding steps repeatedly", () => {
    expect(normalize("pitch", 30)).toBe(24);
    expect(normalize("formant", 0.1)).toBe(0.5);
    let v = 0;
    for (let i = 0; i < 11; i++) v = normalize("pitch", v + 0.1);
    expect(v).toBe(1.1);
  });
});

describe("reading field text", () => {
  it("accepts +, -, and − as signs", () => {
    expect(parseShift("+5")).toBe(5);
    expect(parseShift("−3")).toBe(-3);
    expect(parseShift("-0.5")).toBe(-0.5);
    expect(parseShift(" 1.25 ")).toBe(1.25);
  });

  it("returns null for text that is not a number", () => {
    expect(parseShift("abc")).toBeNull();
    expect(parseShift("")).toBeNull();
    expect(parseShift("1..2")).toBeNull();
  });
});

describe("logarithmic formant slider", () => {
  it("puts ×1 at the centre and ×0.5 and ×2 at the ends", () => {
    expect(sliderToFormant(0)).toBe(1);
    expect(sliderToFormant(-1)).toBe(0.5);
    expect(sliderToFormant(1)).toBe(2);
    expect(formantToSlider(1)).toBe(0);
  });

  it("keeps the value through factor → position → factor", () => {
    for (let r = 0.5; r <= 2; r = normalize("formant", r + 0.01)) {
      expect(sliderToFormant(formantToSlider(r))).toBe(r);
      if (r === 2) break;
    }
  });
});

describe("filled part of the slider", () => {
  it("runs from the centre to the thumb", () => {
    expect(fillRange(-24, 24, 0)).toEqual([50, 50]);
    expect(fillRange(-24, 24, 12)).toEqual([50, 75]);
    expect(fillRange(-24, 24, -12)).toEqual([25, 50]);
  });
});
