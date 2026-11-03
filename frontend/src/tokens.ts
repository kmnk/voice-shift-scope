// Colour values live in the :root variables of style.css. Colours drawn on canvases are read from there each time (never hard-coded).

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

/// The value of a CSS variable. Returns var(--name) where there is no document (unit tests).
export function color(name: ColorToken): string {
  if (typeof document === "undefined") return `var(--${name})`;
  return getComputedStyle(document.documentElement).getPropertyValue(`--${name}`).trim();
}

/// Names of the F1–F4 colour variables
export const FORMANT_TOKENS: ColorToken[] = ["formant-1", "formant-2", "formant-3", "formant-4"];

export function formantColor(i: number): string {
  return color(FORMANT_TOKENS[i]);
}
