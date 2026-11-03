// The cursor over the plots. Draws a dashed line and a value label on a transparent canvas laid over the plot canvas.
// It does not draw on the plot canvas (so it stays out of saved PNGs, and so it does not scroll with the spectrogram).
// The label values are also written to data-cursor-freq and data-cursor-db for the self-test to read.

import { fitCanvas, formatDb, formatKHz, gramFreqAt, spectrumDbAt, spectrumFreqAt, spectrumLayout } from "./render";
import { color } from "./tokens";

type Kind = "gram" | "spectrum";

const AXIS = 34;

/// The value label: white text on a rounded rectangle sized to the text.
function pill(c: CanvasRenderingContext2D, text: string, x: number, y: number, align: "left" | "center", dpr: number, maxRight: number): void {
  c.font = `${10 * dpr}px system-ui`;
  const w = c.measureText(text).width + 8 * dpr;
  const h = 14 * dpr;
  let left = align === "center" ? x - w / 2 : x;
  left = Math.max(0, Math.min(maxRight - w, left));
  const top = Math.max(0, y - h / 2);
  c.globalAlpha = 0.85;
  c.fillStyle = color("ink");
  c.beginPath();
  c.roundRect(left, top, w, h, 3 * dpr);
  c.fill();
  c.globalAlpha = 1;
  c.fillStyle = color("surface");
  c.textAlign = "left";
  c.textBaseline = "middle";
  c.fillText(text, left + 4 * dpr, top + h / 2);
  c.textBaseline = "alphabetic";
}

function dashed(c: CanvasRenderingContext2D, x0: number, y0: number, x1: number, y1: number, dpr: number): void {
  c.strokeStyle = color("muted");
  c.lineWidth = Math.max(1, Math.round(dpr));
  c.setLineDash([3 * dpr, 3 * dpr]);
  c.beginPath();
  c.moveTo(x0, y0);
  c.lineTo(x1, y1);
  c.stroke();
  c.setLineDash([]);
}

export class Cursor {
  private pos: { x: number; y: number } | null = null;

  constructor(
    private overlay: HTMLCanvasElement,
    private kind: Kind,
    private axisMax: () => number,
  ) {
    overlay.addEventListener("pointermove", (e) => this.move(e.offsetX, e.offsetY));
    overlay.addEventListener("pointerleave", () => this.leave());
  }

  move(x: number, y: number): void {
    this.pos = { x, y };
    this.draw();
  }

  leave(): void {
    this.pos = null;
    this.draw();
  }

  /// Also redraws when the size or vertical range changes
  draw(): void {
    const o = this.overlay;
    fitCanvas(o);
    const c = o.getContext("2d")!;
    c.clearRect(0, 0, o.width, o.height);
    delete o.dataset.cursorFreq;
    delete o.dataset.cursorDb;
    if (!this.pos) return;
    const dpr = window.devicePixelRatio || 1;
    const x = this.pos.x * dpr;
    const y = this.pos.y * dpr;
    const axisMax = this.axisMax();
    if (this.kind === "gram") {
      // From the right of the scrolling plot's left scale to just before the value column
      const hz = gramFreqAt(y, o.height, axisMax);
      dashed(c, AXIS * dpr, y, o.width, y, dpr);
      const text = formatKHz(hz);
      pill(c, text, 0, y, "left", dpr, o.width);
      o.dataset.cursorFreq = text;
      return;
    }
    const l = spectrumLayout(o.width, o.height, dpr);
    const hz = spectrumFreqAt(x, l, axisMax);
    const db = spectrumDbAt(y, l);
    if (hz === null || db === null) return;
    dashed(c, x, 0, x, l.plotH, dpr);
    dashed(c, l.plotX, y, l.plotX + l.plotW, y, dpr);
    const hzText = formatKHz(hz);
    const dbText = formatDb(db);
    pill(c, hzText, x, o.height - 9 * dpr, "center", dpr, l.plotX + l.plotW);
    pill(c, dbText, 0, y, "left", dpr, o.width);
    o.dataset.cursorFreq = hzText;
    o.dataset.cursorDb = dbText;
  }
}
