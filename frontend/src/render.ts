// 描画の部品。store の値を読むだけで、状態を持たない（スペクトログラムの過去の画像だけを canvas に持つ）。

import type { Frame } from "./types";
import type { SideView } from "./store";
import type { Summary } from "./summary";

export const FORMANT_COLORS = ["#1f77b4", "#e4572e", "#2a9d8f", "#e9b10a"];
const AXIS = 40;
/// dB を濃さに直す範囲
const DB_FLOOR = -100;
const DB_TOP = -20;

function shade(db: number): number {
  const t = Math.min(1, Math.max(0, (db - DB_FLOOR) / (DB_TOP - DB_FLOOR)));
  return Math.round(255 * (1 - t));
}

/// 狭帯域のスペクトログラム。1 列が分析の 1 フレーム（10 ms）。右から左へ流れる。
export class Spectrogram {
  private ctx: CanvasRenderingContext2D;
  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
    this.clear(5500);
  }

  clear(ceiling: number): void {
    const { width, height } = this.canvas;
    this.ctx.fillStyle = "#fff";
    this.ctx.fillRect(0, 0, width, height);
    this.drawAxis(ceiling);
  }

  private drawAxis(ceiling: number): void {
    const { height } = this.canvas;
    const c = this.ctx;
    c.fillStyle = "#fff";
    c.fillRect(0, 0, AXIS, height);
    c.fillStyle = "#6b6b66";
    c.font = "10px system-ui";
    c.textAlign = "right";
    for (let f = 0; f <= ceiling; f += 1000) {
      const y = height - (f / ceiling) * height;
      c.fillText(`${f / 1000}k`, AXIS - 4, Math.min(height - 2, Math.max(10, y + 3)));
    }
  }

  push(frame: Frame, ceiling: number): void {
    const { width, height } = this.canvas;
    const c = this.ctx;
    c.drawImage(this.canvas, AXIS + 1, 0, width - AXIS - 1, height, AXIS, 0, width - AXIS - 1, height);
    const x = width - 1;
    const column = c.createImageData(1, height);
    const n = frame.spectrum.length;
    for (let y = 0; y < height; y++) {
      const idx = Math.min(n - 1, Math.floor(((height - 1 - y) / height) * n));
      const v = n > 0 ? shade(frame.spectrum[idx]) : 255;
      column.data.set([v, v, v, 255], y * 4);
    }
    c.putImageData(column, x, 0);
    if (frame.voiced) {
      frame.formants.forEach((f, i) => {
        if (!f) return;
        const y = height - (f.frequency / ceiling) * height;
        c.fillStyle = FORMANT_COLORS[i];
        c.fillRect(x - 1, y - 1, 2, 3);
      });
    }
    this.drawAxis(ceiling);
  }
}

/// 今の瞬間のスペクトル。横が周波数、縦が dB。前を灰色、後を青で重ねる。
export function drawSpectrum(canvas: HTMLCanvasElement, before: SideView | null, after: SideView | null, ceiling: number): void {
  const c = canvas.getContext("2d")!;
  const { width, height } = canvas;
  const plotH = height - 18;
  c.fillStyle = "#fff";
  c.fillRect(0, 0, width, height);
  c.strokeStyle = "#eee";
  c.fillStyle = "#6b6b66";
  c.font = "10px system-ui";
  c.textAlign = "center";
  for (let f = 0; f <= ceiling; f += 1000) {
    const x = (f / ceiling) * width;
    c.beginPath();
    c.moveTo(x, 0);
    c.lineTo(x, plotH);
    c.stroke();
    c.fillText(`${f / 1000}k`, Math.min(width - 10, Math.max(10, x)), height - 4);
  }
  const yOf = (db: number) => plotH - ((Math.max(DB_FLOOR, Math.min(0, db)) - DB_FLOOR) / -DB_FLOOR) * plotH;
  const draw = (view: SideView | null, color: string, dash: number[]) => {
    if (!view) return;
    const s = view.frame.spectrum;
    c.strokeStyle = color;
    c.setLineDash(dash);
    c.beginPath();
    s.forEach((db, i) => {
      const x = (i / (s.length - 1)) * width;
      if (i === 0) c.moveTo(x, yOf(db));
      else c.lineTo(x, yOf(db));
    });
    c.stroke();
    c.setLineDash([]);
    view.frame.formants.forEach((f, i) => {
      if (!f || !view.frame.voiced) return;
      const x = (f.frequency / ceiling) * width;
      c.fillStyle = FORMANT_COLORS[i];
      c.fillRect(x - 1, plotH - 10, 3, 10);
    });
    view.frame.harmonics.forEach((h) => {
      const x = (h.frequency / ceiling) * width;
      c.beginPath();
      c.arc(x, yOf(h.level), 2.5, 0, Math.PI * 2);
      c.fillStyle = color;
      c.strokeStyle = color;
      if (h.observed) c.fill();
      else c.stroke();
    });
  };
  draw(before, "#8a8a85", [4, 2]);
  draw(after, "#1f6fd1", []);
}

function fmt(v: number | null | undefined, digits = 0): string {
  return v === null || v === undefined ? "—" : v.toFixed(digits);
}

function ratio(a: number | null | undefined, b: number | null | undefined): string {
  return a && b ? (a / b).toFixed(2) : "—";
}

/// 数値の表の行（PNG にも同じ行を描く）。
export function tableRows(before: Summary | null, after: Summary | null): string[][] {
  const rows: string[][] = [["", "前", "後", "後/前"]];
  rows.push(["F0 (Hz)", fmt(before?.f0, 1), fmt(after?.f0, 1), ratio(after?.f0, before?.f0)]);
  for (let i = 0; i < 4; i++) {
    const b = before?.formants[i];
    const a = after?.formants[i];
    rows.push([`F${i + 1} (Hz)`, fmt(b?.frequency), fmt(a?.frequency), ratio(a?.frequency, b?.frequency)]);
    rows.push([`  帯域幅`, fmt(b?.bandwidth), fmt(a?.bandwidth), ""]);
  }
  for (let k = 0; k < 10; k++) {
    const b = before?.harmonics[k];
    const a = after?.harmonics[k];
    const cell = (h: typeof b) => (h ? `${h.frequency.toFixed(0)} / ${h.level.toFixed(0)} dB${h.observed ? "" : " ×"}` : "—");
    rows.push([`H${k + 1}`, cell(b), cell(a), ratio(a?.frequency, b?.frequency)]);
  }
  rows.push(["観測できた倍音の数", fmt(before?.observedHarmonics), fmt(after?.observedHarmonics), ""]);
  return rows;
}

export function renderTable(table: HTMLTableElement, before: SideView | null, after: SideView | null, timesDiffer: boolean): void {
  const rows = tableRows(before?.summary ?? null, after?.summary ?? null);
  const stale = [before?.summary?.stale, after?.summary?.stale];
  table.innerHTML = rows
    .map((r, i) => {
      const tag = i === 0 ? "th" : "td";
      return `<tr>${r
        .map((cell, j) => {
          const cls = [
            j > 0 && j < 3 && stale[j - 1] ? "stale" : "",
            j === 3 && timesDiffer && cell && cell !== "—" ? "differs" : "",
          ]
            .filter(Boolean)
            .join(" ");
          const mark = j === 3 && i === 0 && timesDiffer ? " ※時刻が違う" : "";
          return `<${tag}${cls ? ` class="${cls}"` : ""}>${cell}${mark}</${tag}>`;
        })
        .join("")}</tr>`;
    })
    .join("");
}
