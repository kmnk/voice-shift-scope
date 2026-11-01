// 止めた側の重ね描き。片側だけを止めているとき、止めた側の F1〜F4 の高さに、動いている側の図の上で横の破線を引く。
// 図の canvas は前の画像を左にずらして描くので、流れない線は図に重ねた別の canvas に描く（保存する PNG には png.ts が重ねる）。
// 描いた値は data-targets に書き、自己試験が読む。目標の値の文字は、図の右端の値の欄（render.ts）が今の値と一緒に描く。

import { AXIS, VALUE_GUTTER, fitCanvas } from "./render";
import { formantColor } from "./tokens";

/// 破線の形（CSS の px）: 6px の線と 4px の間
export const TARGET_DASH = [6, 4];

/// 目標の高さ（上が 0）。値の無いフォルマントは null
export function targetLineYs(values: (number | null)[], axisMax: number, height: number): (number | null)[] {
  return values.map((v) => (v === null || !Number.isFinite(v) || axisMax <= 0 ? null : height - (v / axisMax) * height));
}

export class TargetOverlay {
  private values: (number | null)[] = [];
  private axisMax = 0;

  constructor(private canvas: HTMLCanvasElement) {}

  /// 重ねる値を替える。空なら消す
  set(values: (number | null)[], axisMax: number): void {
    const same = axisMax === this.axisMax && values.length === this.values.length && values.every((v, i) => v === this.values[i]);
    this.values = values;
    this.axisMax = axisMax;
    if (!same) this.draw();
  }

  /// 大きさが変わったときにも描き直す
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
