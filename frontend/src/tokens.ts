// 色の値は style.css の :root の変数が正本。canvas に描く色は、ここから描くたびに読む（直書きしない）。

export type ColorToken =
  | "surface"
  | "ink"
  | "muted"
  | "line"
  | "grid"
  | "accent"
  | "hold"
  | "harmonic"
  | "before"
  | "formant-1"
  | "formant-2"
  | "formant-3"
  | "formant-4";

/// CSS の変数の値。document の無いところ（単体の試験）では var(--name) を返す。
export function color(name: ColorToken): string {
  if (typeof document === "undefined") return `var(--${name})`;
  return getComputedStyle(document.documentElement).getPropertyValue(`--${name}`).trim();
}

/// F1〜F4 の色の変数の名前
export const FORMANT_TOKENS: ColorToken[] = ["formant-1", "formant-2", "formant-3", "formant-4"];

export function formantColor(i: number): string {
  return color(FORMANT_TOKENS[i]);
}
