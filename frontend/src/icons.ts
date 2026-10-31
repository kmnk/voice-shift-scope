// ボタンなどのアイコン。Lucide（ISC ライセンス、https://lucide.dev ）の線のアイコンを SVG で作る。
// 読み上げには出さない（文字で意味を持たせる）。

import {
  ArrowLeftRight,
  Camera,
  CircleQuestionMark,
  Headphones,
  Maximize2,
  Minimize2,
  Play,
  RotateCcw,
  Square,
  TriangleAlert,
  X,
  createElement,
  type IconNode,
} from "lucide";

const ICONS = {
  compact: Minimize2,
  full: Maximize2,
  start: Play,
  stop: Square,
  monitor: Headphones,
  route: ArrowLeftRight,
  reset: RotateCcw,
  snapshot: Camera,
  help: CircleQuestionMark,
  close: X,
  warn: TriangleAlert,
} satisfies Record<string, IconNode>;

export type IconName = keyof typeof ICONS;

export function icon(name: IconName): SVGElement {
  const svg = createElement(ICONS[name], { class: "icon", "aria-hidden": "true", "stroke-width": 1.5 });
  svg.dataset.icon = name;
  return svg;
}

/// ボタンの中身をアイコンと文字にする。文字は .label の span に入れる（data-i18n はそこに付ける）。
export function setContent(el: HTMLElement, name: IconName | null, text: string): void {
  let label = el.querySelector<HTMLElement>(":scope > .label");
  const current = el.querySelector<SVGElement>(":scope > svg.icon");
  if (current?.dataset.icon !== (name ?? undefined)) {
    current?.remove();
    if (name) el.prepend(icon(name));
  }
  if (!label) {
    label = document.createElement("span");
    label.className = "label";
    el.append(label);
  }
  label.textContent = text;
}
