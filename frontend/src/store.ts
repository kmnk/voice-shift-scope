// 表示を止める状態。止めるのは画面の側だけで、分析の側は止めたことを知らない。
// 変える前の側と変えた後の側を、それぞれ「リアルタイム」か「止めている」かで持つ。

import { summarize, type Summary } from "./summary";
import type { Frame, Pair, Settings, Side } from "./types";

/// 数値の表の中央値に使うフレームの数（10 ms ごとなので 250 ms）。
export const SUMMARY_FRAMES = 25;

export interface SideView {
  frozen: boolean;
  /// 表示している時刻（入力を始めてからの秒数）
  time: number;
  frame: Frame;
  summary: Summary | null;
  /// その表示を出した時の設定
  settings: Settings;
}

export interface View {
  before: SideView | null;
  after: SideView | null;
}

export class FreezeStore {
  private recent: Pair[] = [];
  private live: { before: Summary | null; after: Summary | null } = { before: null, after: null };
  private frozen: { before: SideView | null; after: SideView | null } = { before: null, after: null };

  /// 分析の結果を受ける。止めている側には反映しない。
  receive(pair: Pair): void {
    this.recent.push(pair);
    if (this.recent.length > SUMMARY_FRAMES) this.recent.shift();
    for (const side of ["before", "after"] as const) {
      if (!this.frozen[side]) {
        this.live[side] = summarize(
          this.recent.map((p) => p[side]),
          this.live[side],
        );
      }
    }
  }

  isFrozen(side: Side): boolean {
    return this.frozen[side] !== null;
  }

  anyFrozen(): boolean {
    return this.isFrozen("before") || this.isFrozen("after");
  }

  private liveView(side: Side): SideView | null {
    const latest = this.recent[this.recent.length - 1];
    if (!latest) return null;
    return { frozen: false, time: latest.time, frame: latest[side], summary: this.live[side], settings: latest.settings };
  }

  view(side: Side): SideView | null {
    return this.frozen[side] ?? this.liveView(side);
  }

  /// 片方を止める・戻す。戻すと、その時点の最新の表示から再開する。
  toggle(side: Side): void {
    if (this.frozen[side]) {
      this.frozen[side] = null;
      this.live[side] = summarize(
        this.recent.map((p) => p[side]),
        null,
      );
    } else {
      const v = this.liveView(side);
      if (v) this.frozen[side] = { ...v, frozen: true };
    }
  }

  /// 全体の操作: どちらかがリアルタイムなら両方を止め、両方が止まっていれば両方を戻す。
  toggleAll(): void {
    const both = this.isFrozen("before") && this.isFrozen("after");
    for (const side of ["before", "after"] as const) {
      if (both || !this.isFrozen(side)) this.toggle(side);
    }
  }

  /// 前と後で表示している時刻が違うか（比に印を付けるため）。
  timesDiffer(): boolean {
    const b = this.view("before");
    const a = this.view("after");
    return !!b && !!a && Math.abs(b.time - a.time) > 1e-6;
  }
}
