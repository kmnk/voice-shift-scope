// Overlay of the held side. When only one side is held, draws horizontal dashed lines at the held side's F1–F4 heights on the live side's plot.
// The plot canvas is drawn by shifting the previous image left, so lines that do not scroll go on a separate canvas over the plot (png.ts adds it to saved PNGs).
// The drawn values are written to data-targets for the self-test to read. The target value text is drawn by the value column at the plot's right edge (render.ts) along with the current values.

import { AXIS, VALUE_GUTTER, fitCanvas } from "./render";
import { formantColor } from "./tokens";

/// Dash pattern (CSS px): 6px line, 4px gap
export const TARGET_DASH = [6, 4];

/// Target heights (0 at the top). null for formants without a value
export function targetLineYs(values: (number | null)[], axisMax: number, height: number): (number | null)[] {
  return values.map((v) => (v === null || !Number.isFinite(v) || axisMax <= 0 ? null : height - (v / axisMax) * height));
}

export class TargetOverlay {
  private values: (number | null)[] = [];
  private axisMax = 0;

  constructor(private canvas: HTMLCanvasElement) {}

  /// Changes the overlaid values. Clears when empty
  set(values: (number | null)[], axisMax: number): void {
    const same = axisMax === this.axisMax && values.length === this.values.length && values.every((v, i) => v === this.values[i]);
    this.values = values;
    this.axisMax = axisMax;
    if (!same) this.draw();
  }

  /// Also redraws when the size changes
  draw(): void {
    const o = this.canvas;
    fitCanvas(o);
    const c = o.getContext("2d")!;
    c.clearRect(0, 0, o.width, o.height);
    const dpr = window.devicePixelRatio || 1;
    const ys = targetLineYs(this.values, this.axisMax, o.height);
    const left = Math.round(AXIS * dpr);
    const right = o.width - Math.round(VALUE_GUTTER * dpr);
    c.lineWidth = Math.max(1, Math.round(dpr));
    c.setLineDash(TARGET_DASH.map((d) => d * dpr));
    ys.forEach((y, i) => {
      if (y === null) return;
      const yy = Math.round(y) + 0.5;
      c.strokeStyle = formantColor(i);
      c.beginPath();
      c.moveTo(left, yy);
      c.lineTo(right, yy);
      c.stroke();
    });
    c.setLineDash([]);
    o.dataset.targets = this.values.map((v) => (v === null ? "" : v.toFixed(0))).join(",");
  }
}
