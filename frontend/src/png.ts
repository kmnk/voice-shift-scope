// 止めた画面を 1 枚の PNG に描き直す。

import { t } from "./i18n";
import { tableRows } from "./render";
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

export async function composePng(
  grams: [HTMLCanvasElement, HTMLCanvasElement],
  spectrum: HTMLCanvasElement,
  before: SideView | null,
  after: SideView | null,
  now: Date,
): Promise<Uint8Array> {
  const dpr = window.devicePixelRatio || 1;
  // canvas は画素数が画面の拡大率倍になっているので、表示の大きさ（CSS の px）で並べる
  const gw = grams[0].clientWidth;
  const gh = grams[0].clientHeight;
  const sw = spectrum.clientWidth;
  const sh = spectrum.clientHeight;
  const rows = tableRows(before?.summary ?? null, after?.summary ?? null);
  const width = Math.max(gw * 2 + 30, gw + 560);
  const header = 86;
  const height = header + gh + 20 + Math.max(sh, rows.length * 16 + 10) + 20;
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  const c = canvas.getContext("2d")!;
  c.scale(dpr, dpr);
  c.fillStyle = "#fff";
  c.fillRect(0, 0, width, height);
  c.fillStyle = "#1d1d1b";
  c.font = "13px system-ui";
  c.fillText(t("png.title", { date: now.toLocaleString(document.documentElement.lang === "ja" ? "ja-JP" : "en-US") }), 10, 18);
  c.font = "12px system-ui";
  c.fillText(sideLine(t("original"), before), 10, 38);
  c.fillText(sideLine(t("shifted"), after), 10, 56);
  c.font = "bold 12px system-ui";
  c.fillText(t("original"), 10, header - 6);
  c.fillText(t("shifted"), gw + 20, header - 6);
  c.font = "12px system-ui";
  c.drawImage(grams[0], 10, header, gw, gh);
  c.drawImage(grams[1], gw + 20, header, gw, gh);
  const lowerTop = header + gh + 20;
  c.drawImage(spectrum, 10, lowerTop, sw, sh);
  c.font = "12px system-ui";
  const colX = [gw + 20, gw + 170, gw + 350];
  rows.forEach((r, i) => {
    r.forEach((cell, j) => c.fillText(cell, colX[j], lowerTop + 12 + i * 16));
  });
  const blob: Blob = await new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("PNG にできませんでした"))), "image/png"),
  );
  return new Uint8Array(await blob.arrayBuffer());
}
