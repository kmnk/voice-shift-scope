// ピッチとフォルマントの値の書式・読み取り・スライダーの位置。DOM に依らない。
// 値の欄はいつもここの書式で出す（操作によって桁が変わらないように）。

export type ShiftTarget = "pitch" | "formant";

export const RANGE: Record<ShiftTarget, { min: number; max: number }> = {
  pitch: { min: -24, max: 24 },
  formant: { min: 0.5, max: 2 },
};

/// 押す・ホイール・キーで動かす幅。小さい刻みと、Shift と一緒のときの大きい刻み
export const STEP: Record<ShiftTarget, { fine: number; coarse: number }> = {
  pitch: { fine: 0.1, coarse: 1 },
  formant: { fine: 0.01, coarse: 0.05 },
};

const DECIMALS: Record<ShiftTarget, number> = { pitch: 1, formant: 2 };

/// 範囲で止め、桁に丸める（0.1 や 0.01 を足し続けても誤差が残らないように）
export function normalize(target: ShiftTarget, value: number): number {
  const { min, max } = RANGE[target];
  const clamped = Math.min(max, Math.max(min, value));
  const rounded = Number(clamped.toFixed(DECIMALS[target]));
  return Object.is(rounded, -0) ? 0 : rounded;
}

/// ピッチ: 符号つきで小数 1 桁（+5.0、0.0、−3.2）。負はマイナス記号（U+2212）
export function formatPitch(st: number): string {
  const v = normalize("pitch", st);
  const text = Math.abs(v).toFixed(1);
  if (v > 0) return `+${text}`;
  if (v < 0) return `−${text}`;
  return text;
}

/// フォルマント: 小数 2 桁（1.00）
export function formatFormant(ratio: number): string {
  return normalize("formant", ratio).toFixed(2);
}

export function formatShift(target: ShiftTarget, value: number): string {
  return target === "pitch" ? formatPitch(value) : formatFormant(value);
}

/// 欄に入れた文字を数にする。+・-・−（U+2212）を符号として受け、読めなければ null
export function parseShift(text: string): number | null {
  const s = text.trim().replace(/−/g, "-").replace(/,/g, ".");
  if (!/^[+-]?(\d+(\.\d*)?|\.\d+)$/.test(s)) return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

/// フォルマントのスライダーは log2（倍率）を持つ。×0.5 が −1、×1 が 0（中央）、×2 が 1
export function formantToSlider(ratio: number): number {
  return Math.log2(normalize("formant", ratio));
}

/// スライダーの位置から倍率へ。0.01 に丸める
export function sliderToFormant(position: number): number {
  return normalize("formant", 2 ** position);
}

/// スライダーの埋まった部分は、中央（既定の値）からつまみまで。[始まり, 終わり] を 0〜100 の % で返す
export function fillRange(min: number, max: number, value: number): [number, number] {
  if (!(max > min)) return [50, 50];
  const pct = ((Math.min(max, Math.max(min, value)) - min) / (max - min)) * 100;
  return pct >= 50 ? [50, pct] : [pct, 50];
}
