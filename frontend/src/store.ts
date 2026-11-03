// The hold state of the display. Only the screen holds; the analysis side does not know.
// Keeps the original side and the shifted side each as "live" or "held".

import { summarize, type Summary } from "./summary";
import type { Frame, Pair, Settings, Side } from "./types";

/// Number of frames used for the median in the values table (250 ms at 10 ms each).
export const SUMMARY_FRAMES = 25;

export interface SideView {
  frozen: boolean;
  /// Time of the display (seconds since the input started)
  time: number;
  frame: Frame;
  summary: Summary | null;
  /// The settings when that display was produced
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

  /// Takes an analysis result. Not applied to a held side.
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

  /// Holds or resumes one side. Resuming continues from the latest display at that moment.
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

  /// ALL: if either side is live, hold both; if both are held, resume both.
  toggleAll(): void {
    const both = this.isFrozen("before") && this.isFrozen("after");
    for (const side of ["before", "after"] as const) {
      if (both || !this.isFrozen(side)) this.toggle(side);
    }
  }

  /// Whether the original and shifted displays show different times (to mark the ratio).
  timesDiffer(): boolean {
    const b = this.view("before");
    const a = this.view("after");
    return !!b && !!a && Math.abs(b.time - a.time) > 1e-6;
  }
}
