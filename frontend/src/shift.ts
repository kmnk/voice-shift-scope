// Formatting, parsing, and slider positions of the pitch and formant values. Does not depend on the DOM.
// The value fields always use these formats (so the number of digits does not change with the operation).

export type ShiftTarget = "pitch" | "formant";

export const RANGE: Record<ShiftTarget, { min: number; max: number }> = {
  pitch: { min: -24, max: 24 },
  formant: { min: 0.5, max: 2 },
};

/// Steps for buttons, the wheel, and keys: a small step, and a large step with Shift
export const STEP: Record<ShiftTarget, { fine: number; coarse: number }> = {
  pitch: { fine: 0.1, coarse: 1 },
  formant: { fine: 0.01, coarse: 0.05 },
};

const DECIMALS: Record<ShiftTarget, number> = { pitch: 1, formant: 2 };

/// Clamps to the range and rounds to the digits (so adding 0.1 or 0.01 repeatedly leaves no error)
export function normalize(target: ShiftTarget, value: number): number {
  const { min, max } = RANGE[target];
  const clamped = Math.min(max, Math.max(min, value));
  const rounded = Number(clamped.toFixed(DECIMALS[target]));
  return Object.is(rounded, -0) ? 0 : rounded;
}

/// Pitch: signed with 1 decimal place (+5.0, 0.0, −3.2). Negatives use the minus sign (U+2212)
export function formatPitch(st: number): string {
  const v = normalize("pitch", st);
  const text = Math.abs(v).toFixed(1);
  if (v > 0) return `+${text}`;
  if (v < 0) return `−${text}`;
  return text;
}

/// Formant: 2 decimal places (1.00)
export function formatFormant(ratio: number): string {
  return normalize("formant", ratio).toFixed(2);
}

export function formatShift(target: ShiftTarget, value: number): string {
  return target === "pitch" ? formatPitch(value) : formatFormant(value);
}

/// Turns the text in a field into a number. Accepts +, -, and − (U+2212) as signs; null if it cannot be read
export function parseShift(text: string): number | null {
  const s = text.trim().replace(/−/g, "-").replace(/,/g, ".");
  if (!/^[+-]?(\d+(\.\d*)?|\.\d+)$/.test(s)) return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

/// The formant slider holds log2(factor). ×0.5 is −1, ×1 is 0 (the centre), and ×2 is 1
export function formantToSlider(ratio: number): number {
  return Math.log2(normalize("formant", ratio));
}

/// From the slider position to the factor, rounded to 0.01
export function sliderToFormant(position: number): number {
  return normalize("formant", 2 ** position);
}

/// The filled part of a slider runs from the centre (the default value) to the thumb. Returns [start, end] as % from 0 to 100
export function fillRange(min: number, max: number, value: number): [number, number] {
  if (!(max > min)) return [50, 50];
  const pct = ((Math.min(max, Math.max(min, value)) - min) / (max - min)) * 100;
  return pct >= 50 ? [50, pct] : [pct, 50];
}
