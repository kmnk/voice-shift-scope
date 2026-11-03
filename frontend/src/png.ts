// Redraws the held screen into one PNG.

import { t } from "./i18n";
import { tableRows } from "./render";
import { color } from "./tokens";
import type { SideView } from "./store";

export function fileName(now: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `voice-shift-scope-${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}.png`;
}

function sideLine(label: string, v: SideView | null): string {
  if (!v) return t("png.side.none", { label });
  const s = v.settings;
  return t("png.side", {
    label,
    state: v.frozen ? t("png.held") : t("png.live"),
    t: v.time.toFixed(2),
    st: s.semitones.toFixed(1),
    fm: s.formant.toFixed(2),
    ceil: s.ceiling.toFixed(0),
    aceil: s.afterCeiling.toFixed(0),
    th: s.threshold.toFixed(0),
    rl: s.relativeLimit.toFixed(0),
  });
}

/// Layout of the lower row of the image: the spectrum on the left and the values table on the right, without overlap.
/// The table width is measured from the text, and the spectrum fills the rest at the same aspect ratio as on screen.
export function layoutLower(totalWidth: number, tableWidth: number, spectrumAspect: number) {
  const margin = 10;
  const gap = 20;
  const spectrumWidth = Math.max(200, totalWidth - margin * 2 - gap - tableWidth);
  const spectrumHeight = spectrumWidth / spectrumAspect;
  const tableX = margin + spectrumWidth + gap;
  return { spectrumX: margin, spectrumWidth, spectrumHeight, tableX, width: Math.max(totalWidth, tableX + tableWidth + margin) };
}

/// Height of the status and settings line at the top of the image (CSS px). The spectrograms are drawn below it
export const PNG_HEADER = 86;

export async function composePng(
  grams: [HTMLCanvasElement, HTMLCanvasElement],
  /// The overlay canvas (dashed lines when one side is held). Copied on top of the spectrogram
  targets: [HTMLCanvasElement, HTMLCanvasElement],
  spectrum: HTMLCanvasElement,
  before: SideView | null,
  after: SideView | null,
  now: Date,
): Promise<Uint8Array> {
  const dpr = window.devicePixelRatio || 1;
  // Canvases have the screen's scale factor times as many pixels, so lay them out at their display size (CSS px)
  const gw = grams[0].clientWidth;
  const gh = grams[0].clientHeight;
  const rows = tableRows(before?.summary ?? null, after?.summary ?? null);

  // Measure the table column widths from the text to draw
  const measure = document.createElement("canvas").getContext("2d")!;
  measure.font = "12px system-ui";
  const colGap = 16;
  const colWidths = [0, 1, 2].map((j) => Math.max(...rows.map((r) => measure.measureText(r[j] ?? "").width)));
  const tableWidth = colWidths.reduce((a, b) => a + b, 0) + colGap * 2;
  const lower = layoutLower(gw * 2 + 30, tableWidth, spectrum.clientWidth / Math.max(1, spectrum.clientHeight));

  const width = lower.width;
  const header = PNG_HEADER;
  const tableHeight = rows.length * 16 + 10;
  const height = header + gh + 20 + Math.max(lower.spectrumHeight, tableHeight) + 20;
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  const c = canvas.getContext("2d")!;
  c.scale(dpr, dpr);
  c.fillStyle = color("surface");
  c.fillRect(0, 0, width, height);
  c.fillStyle = color("ink");
  c.font = "13px system-ui";
  c.fillText(t("png.title", { date: now.toLocaleString(document.documentElement.lang === "ja" ? "ja-JP" : "en-US") }), 10, 18);
  c.font = "12px system-ui";
  c.fillText(sideLine(t("original"), before), 10, 38);
  c.fillText(sideLine(t("shifted"), after), 10, 56);
  c.font = "bold 12px system-ui";
  c.fillText(t("original"), 10, header - 6);
  c.fillText(t("shifted"), gw + 20, header - 6);
  c.drawImage(grams[0], 10, header, gw, gh);
  c.drawImage(grams[1], gw + 20, header, gw, gh);
  c.drawImage(targets[0], 10, header, gw, gh);
  c.drawImage(targets[1], gw + 20, header, gw, gh);
  const lowerTop = header + gh + 20;
  c.drawImage(spectrum, lower.spectrumX, lowerTop, lower.spectrumWidth, lower.spectrumHeight);
  c.font = "12px system-ui";
  // The first column (headings) is left-aligned; number columns are right-aligned
  const colRight = [0, 1, 2].map((j) => lower.tableX + colWidths.slice(0, j + 1).reduce((a, b) => a + b, 0) + colGap * j);
  rows.forEach((r, i) => {
    const y = lowerTop + 12 + i * 16;
    c.textAlign = "left";
    c.fillText(r[0] ?? "", lower.tableX, y);
    c.textAlign = "right";
    c.fillText(r[1] ?? "", colRight[1], y);
    c.fillText(r[2] ?? "", colRight[2], y);
  });
  c.textAlign = "left";
  const blob: Blob = await new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("could not make a PNG"))), "image/png"),
  );
  return new Uint8Array(await blob.arrayBuffer());
}
