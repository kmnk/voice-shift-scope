// 描画の部品。store の値を読むだけで、状態を持たない（スペクトログラムの過去の画像だけを canvas に持つ）。

import { t } from "./i18n";
import type { Frame } from "./types";
import type { SideView } from "./store";
import type { Summary } from "./summary";

export const FORMANT_COLORS = ["#1f77b4", "#e4572e", "#2a9d8f", "#e9b10a"];
const AXIS = 34;
const DB_FLOOR = -100;

/// canvas の画素数を、表示の大きさと画面の拡大率に合わせる。変わったら true。
export function fitCanvas(canvas: HTMLCanvasElement): boolean {
  // 最小化の間などは表示の大きさが 0 になる。そのときは画素数を替えない（替えると描いたものが消える）
  if (canvas.clientWidth === 0 || canvas.clientHeight === 0) return false;
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
  if (canvas.width === w && canvas.height === h) return false;
  canvas.width = w;
  canvas.height = h;
  return true;
}

/// フォルマントと倍音の位置を、時間とともに右から左へ流す図。
/// 1 列が分析の 1 フレーム（10 ms）。倍音は観測できたものだけを灰色の点で、F1〜F4 は色の点で描く。
/// 観測できていない倍音は描かない（フォルマントと混ざり、前と後の違いが見えにくくなるため）。
/// 音の強さの濃淡は描かない（今の瞬間のスペクトルで見る）。
/// 描き直しのために覚えておくフレームの数（10 ms ごとなので 20 秒）。
const HISTORY = 2000;

export class Spectrogram {
  private ctx: CanvasRenderingContext2D;
  private axisMax = 0;
  /// 描いたフレーム。窓の大きさや縦の範囲が変わったら、これから描き直す
  private history: Frame[] = [];
  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
  }

  /// 大きさか縦の範囲が変わったら、覚えているフレームから描き直す。
  prepare(axisMax: number): void {
    if (fitCanvas(this.canvas) || axisMax !== this.axisMax) {
      this.axisMax = axisMax;
      const { width, height } = this.canvas;
      this.ctx.fillStyle = "#fff";
      this.ctx.fillRect(0, 0, width, height);
      this.drawAxis();
      const dpr = window.devicePixelRatio || 1;
      const columns = Math.ceil((width - AXIS * dpr) / Math.max(1, Math.round(dpr)));
      for (const frame of this.history.slice(-columns)) this.draw(frame);
    }
  }

  push(frame: Frame): void {
    this.history.push(frame);
    if (this.history.length > HISTORY) this.history.splice(0, this.history.length - HISTORY);
    this.draw(frame);
  }

  private drawAxis(): void {
    const { height } = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const c = this.ctx;
    const axis = AXIS * dpr;
    c.fillStyle = "#fff";
    c.fillRect(0, 0, axis, height);
    c.fillStyle = "#6b6b66";
    c.font = `${10 * dpr}px system-ui`;
    c.textAlign = "right";
    c.strokeStyle = "#f0f0ee";
    for (let f = 0; f <= this.axisMax; f += 1000) {
      const y = height - (f / this.axisMax) * height;
      c.fillText(`${f / 1000}k`, axis - 4 * dpr, Math.min(height - 2, Math.max(10 * dpr, y + 3 * dpr)));
    }
  }

  private draw(frame: Frame): void {
    const { width, height } = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const axis = Math.round(AXIS * dpr);
    const step = Math.max(1, Math.round(dpr));
    const c = this.ctx;
    c.drawImage(this.canvas, axis + step, 0, width - axis - step, height, axis, 0, width - axis - step, height);
    const x = width - step;
    c.fillStyle = "#fff";
    c.fillRect(x, 0, step, height);
    // 1 kHz ごとの薄い横線
    c.fillStyle = "#eeeeec";
    for (let f = 1000; f < this.axisMax; f += 1000) c.fillRect(x, Math.round(height - (f / this.axisMax) * height), step, 1);
    if (frame.voiced) {
      const dot = Math.max(2, Math.round(2 * dpr));
      c.fillStyle = "#77776f";
      for (const h of frame.harmonics) {
        if (!h.observed) continue;
        const y = height - (h.frequency / this.axisMax) * height;
        c.fillRect(x, y - dot / 2, step, dot);
      }
      frame.formants.forEach((f, i) => {
        if (!f) return;
        const y = height - (f.frequency / this.axisMax) * height;
        c.fillStyle = FORMANT_COLORS[i];
        c.fillRect(x - step, y - dot, step * 2, dot * 2);
      });
    }
  }
}

/// スペクトルの図の配置（canvas の画素）。曲線は上の図に、F1〜F4 の印は図の下の 2 行の帯
/// （上が変える前、下が変えた後）に、行の名前は右の余白に置く。印を曲線に重ねないため。
export interface SpectrumLayout {
  dpr: number;
  /// 曲線・格子・目盛り・印の横の位置を決める幅（右の余白を除く）
  plotW: number;
  /// 曲線を描く高さ
  plotH: number;
  /// 印の行の上端。[変える前, 変えた後]
  rowTop: [number, number];
  rowH: number;
}

const SPECTRUM_RIGHT = 56;
const SPECTRUM_AXIS = 16;
const MARK_ROW = 8;
const MARK_GAP = 2;
/// 印の帯の分だけ図を低くする（図と帯の間 2px、行 8px ×2、行の間 2px、帯と目盛りの間 2px）
const MARK_STRIP = 2 + MARK_ROW * 2 + MARK_GAP + 2;

export function spectrumLayout(width: number, height: number, dpr: number): SpectrumLayout {
  const plotW = Math.max(1, width - SPECTRUM_RIGHT * dpr);
  const plotH = Math.max(1, height - (SPECTRUM_AXIS + MARK_STRIP) * dpr);
  const top = plotH + 2 * dpr;
  return { dpr, plotW, plotH, rowTop: [top, top + (MARK_ROW + MARK_GAP) * dpr], rowH: MARK_ROW * dpr };
}

export interface Mark {
  x: number;
  y: number;
  w: number;
  h: number;
  color: string;
}

/// 片側の F1〜F4 の印の位置。声の出ていないフレームでは出さない（行は空になる）。
export function formantMarks(frame: Frame | null, side: "before" | "after", axisMax: number, l: SpectrumLayout): Mark[] {
  if (!frame || !frame.voiced) return [];
  const y = l.rowTop[side === "before" ? 0 : 1];
  const marks: Mark[] = [];
  frame.formants.forEach((f, i) => {
    if (!f) return;
    const x = (f.frequency / axisMax) * l.plotW;
    marks.push({ x: x - 1.5 * l.dpr, y, w: 3 * l.dpr, h: l.rowH, color: FORMANT_COLORS[i] });
  });
  return marks;
}

/// 今の瞬間のスペクトル。横が周波数、縦が dB。前を灰色の破線、後を青で重ねる。
/// 前と後でスペクトルの範囲（上限）が違うので、それぞれの範囲で横の位置を決める。
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
  const { plotW, plotH } = layout;
  c.fillStyle = "#fff";
  c.fillRect(0, 0, width, height);
  c.strokeStyle = "#eee";
  c.fillStyle = "#6b6b66";
  c.font = `${10 * dpr}px system-ui`;
  c.textAlign = "center";
  for (let f = 0; f <= axisMax; f += 1000) {
    const x = (f / axisMax) * plotW;
    c.beginPath();
    c.moveTo(x, 0);
    c.lineTo(x, plotH);
    c.stroke();
    c.fillText(`${f / 1000}k`, Math.min(plotW - 10 * dpr, Math.max(10 * dpr, x)), height - 4 * dpr);
  }
  const yOf = (db: number) => plotH - ((Math.max(DB_FLOOR, Math.min(0, db)) - DB_FLOOR) / -DB_FLOOR) * plotH;
  const draw = (view: SideView | null, side: "before" | "after", color: string, dash: number[]) => {
    if (!view) return;
    const s = view.frame.spectrum;
    const maxHz = side === "before" ? view.settings.ceiling : view.settings.afterCeiling;
    const xOf = (hz: number) => (hz / axisMax) * plotW;
    c.strokeStyle = color;
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
      c.fillStyle = color;
      c.strokeStyle = color;
      if (h.observed) c.fill();
      else c.stroke();
    });
  };
  draw(before, "before", "#8a8a85", [4, 2]);
  draw(after, "after", "#1f6fd1", []);
  // 右の余白に、印の行の名前を書く（印が無いときも出す）
  c.textAlign = "left";
  c.textBaseline = "middle";
  const rows: [string, string][] = [
    [t("original"), "#8a8a85"],
    [t("shifted"), "#1f6fd1"],
  ];
  rows.forEach(([label, color], i) => {
    c.fillStyle = color;
    c.fillText(label, plotW + 4 * dpr, layout.rowTop[i] + layout.rowH / 2);
  });
  c.textBaseline = "alphabetic";
}

function fmt(v: number | null | undefined, digits = 0): string {
  return v === null || v === undefined ? "—" : v.toFixed(digits);
}

/// 数値の表の行（PNG にも同じ行を描く）。前を左、後を右に置く。比の列は持たない（変えた量は設定で分かるため）。
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

export function renderTable(table: HTMLTableElement, before: SideView | null, after: SideView | null): void {
  const rows = tableRows(before?.summary ?? null, after?.summary ?? null);
  const stale = [before?.summary?.stale, after?.summary?.stale];
  table.innerHTML = rows
    .map((r, i) => {
      const tag = i === 0 ? "th" : "td";
      return `<tr>${r
        .map((cell, j) => `<${tag}${j > 0 && stale[j - 1] ? ' class="stale"' : ""}>${cell}</${tag}>`)
        .join("")}</tr>`;
    })
    .join("");
}
