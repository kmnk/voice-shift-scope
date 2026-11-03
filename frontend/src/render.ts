// Drawing parts. They only read values from the store and keep no state (only the spectrograms keep their past image on the canvas).
// Colours are read from CSS variables through tokens.ts and never hard-coded.

import { t } from "./i18n";
import { color, formantColor } from "./tokens";
import type { Frame } from "./types";
import type { SideView } from "./store";
import type { Summary } from "./summary";

/// Width of the frequency scale on the left of a spectrogram (CSS px)
export const AXIS = 34;
/// Width of the column on the right of a spectrogram that shows the current F1–F4 values (CSS px)
export const VALUE_GUTTER = 44;
/// Minimum gap between values in the value column (CSS px)
export const VALUE_MIN_GAP = 12;
/// Height of the harmonic dots (CSS px). Smaller than F1–F4
export const HARMONIC_PX = 1;
const DB_FLOOR = -100;

/// Matches a canvas's pixel count to its display size and the screen's scale factor. true if it changed.
export function fitCanvas(canvas: HTMLCanvasElement): boolean {
  // The display size becomes 0 while minimised and so on. Then the pixel count is not changed (changing it would erase the drawing)
  if (canvas.clientWidth === 0 || canvas.clientHeight === 0) return false;
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
  if (canvas.width === w && canvas.height === h) return false;
  canvas.width = w;
  canvas.height = h;
  return true;
}

export interface ValueLabel {
  /// F1–F4 index (from 0)
  index: number;
  /// Height of the line (before moving)
  lineY: number;
  /// Height of the text (after moving apart to avoid overlap)
  y: number;
  text: string;
  /// A target value from the held side (overlay). false for current values
  target: boolean;
}

/// Positions of the F1–F4 values in the value column. The height is the frequency position; values too close together push the lower one down.
/// Values beyond the bottom of the plot are moved to the bottom (they may overlap). Formants without a value are not shown.
/// targets (the held side's values when only one side is held) are placed together under the same rules, without overlapping the current values.
export function valueLabels(values: (number | null)[], axisMax: number, height: number, minGap: number, targets: (number | null)[] = []): ValueLabel[] {
  const labels: ValueLabel[] = [];
  const add = (list: (number | null)[], target: boolean) =>
    list.forEach((v, index) => {
      if (v === null || !Number.isFinite(v)) return;
      const lineY = height - (v / axisMax) * height;
      labels.push({ index, lineY, y: lineY, text: v.toFixed(0), target });
    });
  add(values, false);
  add(targets, true);
  labels.sort((a, b) => a.y - b.y);
  const top = minGap / 2;
  const bottom = height - minGap / 2;
  let prev = -Infinity;
  for (const l of labels) {
    l.y = Math.min(bottom, Math.max(top, l.y, prev + minGap));
    prev = l.y;
  }
  return labels;
}

/// A plot that scrolls the formant and harmonic positions from right to left over time.
/// One column is one analysis frame (10 ms). Only harmonics that were seen are drawn, as faint dots; F1–F4 are coloured dots.
/// Harmonics that were not seen are not drawn (they would mix with the formants and make the original/shifted difference harder to see).
/// Sound intensity shading is not drawn (it is seen in the current spectrum).
/// The value column at the right edge does not scroll; it is repainted each time with the current F1–F4 (the same 250 ms medians as the values table).
/// Number of frames kept for redrawing (20 seconds at 10 ms each).
const HISTORY = 2000;

export class Spectrogram {
  private ctx: CanvasRenderingContext2D;
  private axisMax = 0;
  /// Drawn frames. When the window size or vertical range changes, the plot is redrawn from these
  private history: Frame[] = [];
  private values: (number | null)[] = [null, null, null, null];
  private stale = false;
  private targets: (number | null)[] = [];
  private targetsStale = false;
  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
  }

  /// When the size or vertical range changes, redraws from the kept frames.
  prepare(axisMax: number): void {
    if (fitCanvas(this.canvas) || axisMax !== this.axisMax) {
      this.axisMax = axisMax;
      const { width, height } = this.canvas;
      this.ctx.fillStyle = color("surface");
      this.ctx.fillRect(0, 0, width, height);
      this.drawAxis();
      const dpr = window.devicePixelRatio || 1;
      const columns = Math.ceil((width - (AXIS + VALUE_GUTTER) * dpr) / Math.max(1, Math.round(dpr)));
      for (const frame of this.history.slice(-columns)) this.draw(frame);
      this.drawValues();
    }
  }

  push(frame: Frame): void {
    this.history.push(frame);
    if (this.history.length > HISTORY) this.history.splice(0, this.history.length - HISTORY);
    this.draw(frame);
  }

  /// Changes the values at the right edge (a held side is called with the values from when it was held).
  /// targets are the held side's values (goals) when only the other side is held. Empty when not overlaid
  setValues(values: (number | null)[], stale: boolean, targets: (number | null)[] = [], targetsStale = false): void {
    const eq = (a: (number | null)[], b: (number | null)[]) => a.length === b.length && a.every((v, i) => v === b[i]);
    const same = stale === this.stale && targetsStale === this.targetsStale && eq(values, this.values) && eq(targets, this.targets);
    this.values = values;
    this.stale = stale;
    this.targets = targets;
    this.targetsStale = targetsStale;
    if (!same) this.drawValues();
  }

  private drawAxis(): void {
    const { height } = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const c = this.ctx;
    const axis = AXIS * dpr;
    c.fillStyle = color("surface");
    c.fillRect(0, 0, axis, height);
    c.fillStyle = color("muted");
    c.font = `${10 * dpr}px system-ui`;
    c.textAlign = "right";
    for (let f = 0; f <= this.axisMax; f += 1000) {
      const y = height - (f / this.axisMax) * height;
      c.fillText(`${f / 1000}k`, axis - 4 * dpr, Math.min(height - 2, Math.max(10 * dpr, y + 3 * dpr)));
    }
  }

  private drawValues(): void {
    const { width, height } = this.canvas;
    if (this.axisMax <= 0) return;
    const dpr = window.devicePixelRatio || 1;
    const c = this.ctx;
    const left = width - Math.round(VALUE_GUTTER * dpr);
    c.fillStyle = color("surface");
    c.fillRect(left, 0, width - left, height);
    const labels = valueLabels(this.values, this.axisMax, height / dpr, VALUE_MIN_GAP, this.targets);
    c.font = `${10 * dpr}px system-ui`;
    c.textAlign = "left";
    c.textBaseline = "middle";
    const muted = color("muted");
    const hold = color("hold");
    for (const l of labels) {
      if (l.target) {
        // Target values: purple text only (no short line in front). Old values are grey
        c.fillStyle = this.targetsStale ? muted : hold;
        c.fillText(l.text, left + 10 * dpr, l.y * dpr);
        continue;
      }
      const ink = this.stale ? muted : formantColor(l.index);
      c.fillStyle = ink;
      c.fillRect(left + 2 * dpr, Math.round(l.lineY * dpr) - Math.round(dpr / 2), 6 * dpr, Math.max(1, Math.round(dpr)));
      c.fillText(l.text, left + 10 * dpr, l.y * dpr);
    }
    c.textBaseline = "alphabetic";
    // For the self-test to read (text inside the plot cannot be read from the DOM)
    this.canvas.dataset.values = this.values.map((v) => (v === null ? "" : v.toFixed(0))).join(",");
  }

  private draw(frame: Frame): void {
    const { width, height } = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const axis = Math.round(AXIS * dpr);
    const right = width - Math.round(VALUE_GUTTER * dpr);
    const step = Math.max(1, Math.round(dpr));
    const c = this.ctx;
    // Only the part between the scale and the value column scrolls
    c.drawImage(this.canvas, axis + step, 0, right - axis - step, height, axis, 0, right - axis - step, height);
    const x = right - step;
    c.fillStyle = color("surface");
    c.fillRect(x, 0, step, height);
    // Faint horizontal lines every 1 kHz
    c.fillStyle = color("grid");
    for (let f = 1000; f < this.axisMax; f += 1000) c.fillRect(x, Math.round(height - (f / this.axisMax) * height), step, 1);
    if (frame.voiced) {
      const harmonic = Math.max(1, Math.round(HARMONIC_PX * dpr));
      c.fillStyle = color("harmonic");
      for (const h of frame.harmonics) {
        if (!h.observed) continue;
        const y = Math.round(height - (h.frequency / this.axisMax) * height);
        c.fillRect(x, y, step, harmonic);
      }
      const dot = Math.max(2, Math.round(2 * dpr));
      frame.formants.forEach((f, i) => {
        if (!f) return;
        const y = height - (f.frequency / this.axisMax) * height;
        c.fillStyle = formantColor(i);
        c.fillRect(x - step, y - dot, step * 2, dot * 2);
      });
    }
  }
}

/// Layout of the spectrum plot (canvas pixels): a dB scale on the left, the curve in the upper plot, and the F1–F4 marks in a 2-row band below the plot
/// (original on top, shifted below), with the row names in the right margin. This keeps the marks off the curve.
export interface SpectrumLayout {
  dpr: number;
  /// Left edge of the curve, grid, and marks (right of the dB scale)
  plotX: number;
  /// Width that sets the horizontal positions of the curve, grid, scale, and marks (excluding the left scale and the right margin)
  plotW: number;
  /// Height where the curve is drawn
  plotH: number;
  /// Top of the mark rows. [original, shifted]
  rowTop: [number, number];
  rowH: number;
}

const SPECTRUM_LEFT = 34;
const SPECTRUM_RIGHT = 56;
const SPECTRUM_AXIS = 16;
const MARK_ROW = 8;
const MARK_GAP = 2;
/// Lowers the plot by the height of the mark band (2px between the plot and the band, 8px rows ×2, 2px between rows, 2px between the band and the scale)
const MARK_STRIP = 2 + MARK_ROW * 2 + MARK_GAP + 2;

export function spectrumLayout(width: number, height: number, dpr: number): SpectrumLayout {
  const plotX = SPECTRUM_LEFT * dpr;
  const plotW = Math.max(1, width - (SPECTRUM_LEFT + SPECTRUM_RIGHT) * dpr);
  const plotH = Math.max(1, height - (SPECTRUM_AXIS + MARK_STRIP) * dpr);
  const top = plotH + 2 * dpr;
  return { dpr, plotX, plotW, plotH, rowTop: [top, top + (MARK_ROW + MARK_GAP) * dpr], rowH: MARK_ROW * dpr };
}

/// Vertical scale of the spectrum (dB). Every 20 dB from 0 to the floor
export function dbTicks(): number[] {
  const ticks: number[] = [];
  for (let db = 0; db >= DB_FLOOR; db -= 20) ticks.push(db);
  return ticks;
}

/// Negative numbers use the minus sign (U+2212) on the scale and labels
const signed = (n: number) => (n < 0 ? `−${Math.abs(n)}` : String(n));

export function dbLabel(db: number): string {
  return signed(db);
}

/// Cursor label: frequency in kHz with 2 decimal places
export function formatKHz(hz: number): string {
  return `${(hz / 1000).toFixed(2)} kHz`;
}

/// Cursor label: dB as an integer
export function formatDb(db: number): string {
  return `${signed(Math.round(db) === 0 ? 0 : Math.round(db))} dB`;
}

/// Frequency at height y of a spectrogram (0 at the top)
export function gramFreqAt(y: number, height: number, axisMax: number): number {
  return Math.min(axisMax, Math.max(0, (1 - y / height) * axisMax));
}

/// Frequency at horizontal position x of the spectrum. null outside the curve
export function spectrumFreqAt(x: number, l: SpectrumLayout, axisMax: number): number | null {
  if (x < l.plotX || x > l.plotX + l.plotW) return null;
  return ((x - l.plotX) / l.plotW) * axisMax;
}

/// dB at height y of the spectrum. null outside the curve
export function spectrumDbAt(y: number, l: SpectrumLayout): number | null {
  if (y < 0 || y > l.plotH) return null;
  return DB_FLOOR + ((l.plotH - y) / l.plotH) * -DB_FLOOR;
}

export interface Mark {
  x: number;
  y: number;
  w: number;
  h: number;
  color: string;
}

/// Positions of one side's F1–F4 marks. Not shown for frames without voice (the row is empty).
export function formantMarks(frame: Frame | null, side: "before" | "after", axisMax: number, l: SpectrumLayout): Mark[] {
  if (!frame || !frame.voiced) return [];
  const y = l.rowTop[side === "before" ? 0 : 1];
  const marks: Mark[] = [];
  frame.formants.forEach((f, i) => {
    if (!f) return;
    const x = l.plotX + (f.frequency / axisMax) * l.plotW;
    marks.push({ x: x - 1.5 * l.dpr, y, w: 3 * l.dpr, h: l.rowH, color: formantColor(i) });
  });
  return marks;
}

/// The current spectrum. Frequency across, dB up. The original as a grey dashed line, the shifted in blue, overlaid.
/// The original and shifted spectra have different ranges (ceilings), so each uses its own range for horizontal positions.
export function drawSpectrum(
  canvas: HTMLCanvasElement,
  before: SideView | null,
  after: SideView | null,
  axisMax: number,
): void {
  fitCanvas(canvas);
  const c = canvas.getContext("2d")!;
  const { width, height } = canvas;
  const dpr = window.devicePixelRatio || 1;
  const layout = spectrumLayout(width, height, dpr);
  const { plotX, plotW, plotH } = layout;
  const muted = color("muted");
  c.fillStyle = color("surface");
  c.fillRect(0, 0, width, height);
  c.lineWidth = 1;
  c.strokeStyle = color("grid");
  c.fillStyle = muted;
  c.font = `${10 * dpr}px system-ui`;
  // Vertical scale (dB) and horizontal grid
  const yOf = (db: number) => plotH - ((Math.max(DB_FLOOR, Math.min(0, db)) - DB_FLOOR) / -DB_FLOOR) * plotH;
  c.textAlign = "right";
  c.textBaseline = "middle";
  const ticks = dbTicks();
  for (const db of ticks) {
    const y = Math.round(yOf(db)) + 0.5;
    c.beginPath();
    c.moveTo(plotX, y);
    c.lineTo(plotX + plotW, y);
    c.stroke();
    c.fillText(dbLabel(db), plotX - 4 * dpr, Math.min(plotH - 5 * dpr, Math.max(12 * dpr, y)));
  }
  c.textAlign = "left";
  c.textBaseline = "top";
  c.fillText("dB", 2 * dpr, 0);
  c.textBaseline = "alphabetic";
  canvas.dataset.dbTicks = ticks.join(",");
  // Horizontal scale (frequency) and vertical grid
  c.textAlign = "center";
  for (let f = 0; f <= axisMax; f += 1000) {
    const x = plotX + (f / axisMax) * plotW;
    c.beginPath();
    c.moveTo(x, 0);
    c.lineTo(x, plotH);
    c.stroke();
    c.fillText(`${f / 1000}k`, Math.min(plotX + plotW - 10 * dpr, Math.max(plotX + 10 * dpr, x)), height - 4 * dpr);
  }
  const draw = (view: SideView | null, side: "before" | "after", ink: string, dash: number[]) => {
    if (!view) return;
    const s = view.frame.spectrum;
    const maxHz = side === "before" ? view.settings.ceiling : view.settings.afterCeiling;
    const xOf = (hz: number) => plotX + (hz / axisMax) * plotW;
    c.strokeStyle = ink;
    c.lineWidth = dpr;
    c.setLineDash(dash.map((d) => d * dpr));
    c.beginPath();
    s.forEach((db, i) => {
      const x = xOf(((i + 0.5) / s.length) * maxHz);
      if (i === 0) c.moveTo(x, yOf(db));
      else c.lineTo(x, yOf(db));
    });
    c.stroke();
    c.setLineDash([]);
    for (const m of formantMarks(view.frame, side, axisMax, layout)) {
      c.fillStyle = m.color;
      c.fillRect(m.x, m.y, m.w, m.h);
    }
    view.frame.harmonics.forEach((h) => {
      c.beginPath();
      c.arc(xOf(h.frequency), yOf(h.level), 2.5 * dpr, 0, Math.PI * 2);
      c.fillStyle = ink;
      c.strokeStyle = ink;
      if (h.observed) c.fill();
      else c.stroke();
    });
  };
  const beforeInk = color("before");
  const afterInk = color("accent");
  draw(before, "before", beforeInk, [4, 2]);
  draw(after, "after", afterInk, []);
  // Write the mark row names in the right margin (even when there are no marks)
  c.textAlign = "left";
  c.textBaseline = "middle";
  const rows: [string, string][] = [
    [t("original"), beforeInk],
    [t("shifted"), afterInk],
  ];
  rows.forEach(([label, ink], i) => {
    c.fillStyle = ink;
    c.fillText(label, plotX + plotW + 4 * dpr, layout.rowTop[i] + layout.rowH / 2);
  });
  c.textBaseline = "alphabetic";
}

function fmt(v: number | null | undefined, digits = 0): string {
  return v === null || v === undefined ? "—" : v.toFixed(digits);
}

/// Rows of the values table (the PNG draws the same rows). Original on the left, shifted on the right. No ratio column (the amount changed is known from the settings).
export function tableRows(before: Summary | null, after: Summary | null): string[][] {
  const rows: string[][] = [["", t("original"), t("shifted")]];
  rows.push([t("row.f0"), fmt(before?.f0, 1), fmt(after?.f0, 1)]);
  for (let i = 0; i < 4; i++) {
    const cell = (f: { frequency: number; bandwidth: number } | null | undefined) =>
      f ? `${f.frequency.toFixed(0)} (${f.bandwidth.toFixed(0)})` : "—";
    rows.push([t("row.formant", { n: i + 1 }), cell(before?.formants[i]), cell(after?.formants[i])]);
  }
  for (let k = 0; k < 10; k++) {
    const cell = (h: { frequency: number; level: number; observed: boolean } | null | undefined) =>
      h ? `${h.frequency.toFixed(0)} / ${h.level.toFixed(0)} dB${h.observed ? "" : " ×"}` : "—";
    rows.push([`H${k + 1}`, cell(before?.harmonics[k]), cell(after?.harmonics[k])]);
  }
  rows.push([t("row.observed"), fmt(before?.observedHarmonics), fmt(after?.observedHarmonics)]);
  return rows;
}

/// Puts a swatch of the plot colour at the start of the F1–F4 rows of the table (rows 2 to 5 of tableRows)
const FORMANT_ROW_FIRST = 2;

export function renderTable(table: HTMLTableElement, before: SideView | null, after: SideView | null): void {
  const rows = tableRows(before?.summary ?? null, after?.summary ?? null);
  const stale = [before?.summary?.stale, after?.summary?.stale];
  table.innerHTML = rows
    .map((r, i) => {
      const tag = i === 0 ? "th" : "td";
      const formant = i - FORMANT_ROW_FIRST;
      const swatch =
        formant >= 0 && formant < 4
          ? `<span class="swatch" data-formant="${formant + 1}" style="background: var(--formant-${formant + 1})"></span>`
          : "";
      return `<tr>${r
        .map((cell, j) => `<${tag}${j > 0 && stale[j - 1] ? ' class="stale"' : ""}>${j === 0 ? swatch : ""}${cell}</${tag}>`)
        .join("")}</tr>`;
    })
    .join("");
}
