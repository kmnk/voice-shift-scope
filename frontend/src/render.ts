// 描画の部品。store の値を読むだけで、状態を持たない（スペクトログラムの過去の画像だけを canvas に持つ）。
// 色は tokens.ts から CSS の変数を読み、直書きしない。

import { t } from "./i18n";
import { color, formantColor } from "./tokens";
import type { Frame } from "./types";
import type { SideView } from "./store";
import type { Summary } from "./summary";

/// スペクトログラムの左の周波数の目盛りの幅（CSS の px）
export const AXIS = 34;
/// スペクトログラムの右の、F1〜F4 の今の値を出す欄の幅（CSS の px）
export const VALUE_GUTTER = 44;
/// 値の欄で、値どうしを離す最小の間（CSS の px）
export const VALUE_MIN_GAP = 12;
/// 倍音の点の高さ（CSS の px）。F1〜F4 より控えめにする
export const HARMONIC_PX = 1;
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

export interface ValueLabel {
  /// F1〜F4 の番号（0 から）
  index: number;
  /// 線の高さ（ずらす前）
  lineY: number;
  /// 文字の高さ（重ならないようにずらした後）
  y: number;
  text: string;
  /// 止めた側の目標の値（重ね描き）。今の値は false
  target: boolean;
}

/// 値の欄の F1〜F4 の値の位置。高さは周波数の位置にし、近すぎる値は下の値を下へずらす。
/// 図の下端を越えるときは下端に寄せる（重なってよい）。値の無いフォルマントは出さない。
/// targets（片側だけを止めたときの、止めた側の値）も同じ規則で一緒に並べ、今の値と重ねない。
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

/// フォルマントと倍音の位置を、時間とともに右から左へ流す図。
/// 1 列が分析の 1 フレーム（10 ms）。倍音は観測できたものだけを薄い点で、F1〜F4 は色の点で描く。
/// 観測できていない倍音は描かない（フォルマントと混ざり、前と後の違いが見えにくくなるため）。
/// 音の強さの濃淡は描かない（今の瞬間のスペクトルで見る）。
/// 右端の値の欄は流さずに毎回塗り直し、F1〜F4 の今の値（数値の表と同じ 250 ms の中央値）を出す。
/// 描き直しのために覚えておくフレームの数（10 ms ごとなので 20 秒）。
const HISTORY = 2000;

export class Spectrogram {
  private ctx: CanvasRenderingContext2D;
  private axisMax = 0;
  /// 描いたフレーム。窓の大きさや縦の範囲が変わったら、これから描き直す
  private history: Frame[] = [];
  private values: (number | null)[] = [null, null, null, null];
  private stale = false;
  private targets: (number | null)[] = [];
  private targetsStale = false;
  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
  }

  /// 大きさか縦の範囲が変わったら、覚えているフレームから描き直す。
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

  /// 右端の値を替える（止めている側は止めた時点の値のまま呼ばれる）。
  /// targets は、他方だけを止めているときの止めた側の値（目標）。重ねないときは空
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
        // 目標の値: 紫の文字だけ（頭の短い横線は付けない）。古い値は灰色
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
    // 自己試験が読む（図の中の文字は DOM から読めないため）
    this.canvas.dataset.values = this.values.map((v) => (v === null ? "" : v.toFixed(0))).join(",");
  }

  private draw(frame: Frame): void {
    const { width, height } = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const axis = Math.round(AXIS * dpr);
    const right = width - Math.round(VALUE_GUTTER * dpr);
    const step = Math.max(1, Math.round(dpr));
    const c = this.ctx;
    // 流すのは目盛りと値の欄の間だけ
    c.drawImage(this.canvas, axis + step, 0, right - axis - step, height, axis, 0, right - axis - step, height);
    const x = right - step;
    c.fillStyle = color("surface");
    c.fillRect(x, 0, step, height);
    // 1 kHz ごとの薄い横線
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

/// スペクトルの図の配置（canvas の画素）。左に dB の目盛り、曲線は上の図に、F1〜F4 の印は図の下の 2 行の帯
/// （上が変える前、下が変えた後）に、行の名前は右の余白に置く。印を曲線に重ねないため。
export interface SpectrumLayout {
  dpr: number;
  /// 曲線・格子・印の左端（dB の目盛りの右）
  plotX: number;
  /// 曲線・格子・目盛り・印の横の位置を決める幅（左の目盛りと右の余白を除く）
  plotW: number;
  /// 曲線を描く高さ
  plotH: number;
  /// 印の行の上端。[変える前, 変えた後]
  rowTop: [number, number];
  rowH: number;
}

const SPECTRUM_LEFT = 34;
const SPECTRUM_RIGHT = 56;
const SPECTRUM_AXIS = 16;
const MARK_ROW = 8;
const MARK_GAP = 2;
/// 印の帯の分だけ図を低くする（図と帯の間 2px、行 8px ×2、行の間 2px、帯と目盛りの間 2px）
const MARK_STRIP = 2 + MARK_ROW * 2 + MARK_GAP + 2;

export function spectrumLayout(width: number, height: number, dpr: number): SpectrumLayout {
  const plotX = SPECTRUM_LEFT * dpr;
  const plotW = Math.max(1, width - (SPECTRUM_LEFT + SPECTRUM_RIGHT) * dpr);
  const plotH = Math.max(1, height - (SPECTRUM_AXIS + MARK_STRIP) * dpr);
  const top = plotH + 2 * dpr;
  return { dpr, plotX, plotW, plotH, rowTop: [top, top + (MARK_ROW + MARK_GAP) * dpr], rowH: MARK_ROW * dpr };
}

/// スペクトルの縦の目盛り（dB）。0 から下限まで 20 dB ごと
export function dbTicks(): number[] {
  const ticks: number[] = [];
  for (let db = 0; db >= DB_FLOOR; db -= 20) ticks.push(db);
  return ticks;
}

/// 負の数の記号は、目盛りと札でマイナス記号（U+2212）にする
const signed = (n: number) => (n < 0 ? `−${Math.abs(n)}` : String(n));

export function dbLabel(db: number): string {
  return signed(db);
}

/// カーソルの値の札: 周波数は kHz で小数 2 桁
export function formatKHz(hz: number): string {
  return `${(hz / 1000).toFixed(2)} kHz`;
}

/// カーソルの値の札: dB は整数
export function formatDb(db: number): string {
  return `${signed(Math.round(db) === 0 ? 0 : Math.round(db))} dB`;
}

/// スペクトログラムの高さ y（上が 0）の周波数
export function gramFreqAt(y: number, height: number, axisMax: number): number {
  return Math.min(axisMax, Math.max(0, (1 - y / height) * axisMax));
}

/// スペクトルの横の位置 x の周波数。曲線の範囲の外は null
export function spectrumFreqAt(x: number, l: SpectrumLayout, axisMax: number): number | null {
  if (x < l.plotX || x > l.plotX + l.plotW) return null;
  return ((x - l.plotX) / l.plotW) * axisMax;
}

/// スペクトルの高さ y の dB。曲線の範囲の外は null
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

/// 片側の F1〜F4 の印の位置。声の出ていないフレームでは出さない（行は空になる）。
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
  const { plotX, plotW, plotH } = layout;
  const muted = color("muted");
  c.fillStyle = color("surface");
  c.fillRect(0, 0, width, height);
  c.lineWidth = 1;
  c.strokeStyle = color("grid");
  c.fillStyle = muted;
  c.font = `${10 * dpr}px system-ui`;
  // 縦の目盛り（dB）と横の格子
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
  // 横の目盛り（周波数）と縦の格子
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
  // 右の余白に、印の行の名前を書く（印が無いときも出す）
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

/// 表の F1〜F4 の行（tableRows の 2〜5 行目）の頭に、図と同じ色の見本を置く
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
