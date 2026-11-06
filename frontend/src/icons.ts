// Icons for buttons and so on. Made as SVG from Lucide line icons (ISC license, https://lucide.dev ).
// Hidden from screen readers (the text carries the meaning).

import {
  ArrowLeftRight,
  Camera,
  CircleQuestionMark,
  Headphones,
  Lock,
  LockOpen,
  Maximize2,
  Minimize2,
  Play,
  RotateCcw,
  Settings,
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
  settings: Settings,
  close: X,
  warn: TriangleAlert,
  lock: Lock,
  unlock: LockOpen,
} satisfies Record<string, IconNode>;

export type IconName = keyof typeof ICONS;

export function icon(name: IconName): SVGElement {
  const svg = createElement(ICONS[name], { class: "icon", "aria-hidden": "true", "stroke-width": 1.5 });
  svg.dataset.icon = name;
  return svg;
}

/// Makes a button's content an icon and text. The text goes in a .label span (which gets data-i18n).
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
