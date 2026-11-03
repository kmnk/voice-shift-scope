import { describe, expect, it } from "vitest";
import { FreezeStore } from "./store";
import type { Frame, Pair } from "./types";

function frame(f0: number, f1: number): Frame {
  return {
    f0,
    voiced: true,
    formants: [{ frequency: f1, bandwidth: 80 }, null, null, null],
    harmonics: [{ number: 1, frequency: f0, level: -20, observed: true }],
    observedHarmonics: 1,
    spectrum: [],
  };
}

function pair(time: number, beforeF0: number, afterF0: number, semitones = 0): Pair {
  return {
    time,
    before: frame(beforeF0, 700),
    after: frame(afterF0, 700),
    settings: { semitones, formant: 1, ceiling: 5500, afterCeiling: 5500, threshold: 10, relativeLimit: 40 },
  };
}

describe("FreezeStore", () => {
  it("B5: holding all keeps both unchanged, and doing it again returns to the latest", () => {
    const s = new FreezeStore();
    s.receive(pair(1, 150, 200));
    s.toggleAll();
    s.receive(pair(2, 120, 160));
    expect(s.view("before")!.time).toBe(1);
    expect(s.view("after")!.time).toBe(1);
    expect(s.view("before")!.summary!.f0).toBe(150);
    s.toggleAll();
    expect(s.view("before")!.time).toBe(2);
    expect(s.view("after")!.frame.f0).toBe(160);
  });

  it("B7: holding only shifted keeps original moving and marks the times as different", () => {
    const s = new FreezeStore();
    s.receive(pair(1, 150, 200));
    s.toggle("after");
    s.receive(pair(2, 120, 160));
    expect(s.view("after")!.frame.f0).toBe(200);
    expect(s.view("before")!.frame.f0).toBe(120);
    expect(s.timesDiffer()).toBe(true);
    s.toggle("after");
    expect(s.view("after")!.frame.f0).toBe(160);
    expect(s.timesDiffer()).toBe(false);
  });

  it("B7: the same when holding only original", () => {
    const s = new FreezeStore();
    s.receive(pair(1, 150, 200));
    s.toggle("before");
    s.receive(pair(2, 120, 160));
    expect(s.view("before")!.frame.f0).toBe(150);
    expect(s.view("after")!.frame.f0).toBe(160);
  });

  it("B8: ALL after holding one side holds both, and again resumes both", () => {
    const s = new FreezeStore();
    s.receive(pair(1, 150, 200));
    s.toggle("after");
    s.receive(pair(2, 120, 160));
    s.toggleAll();
    expect(s.isFrozen("before") && s.isFrozen("after")).toBe(true);
    expect(s.view("after")!.time).toBe(1);
    expect(s.view("before")!.time).toBe(2);
    s.receive(pair(3, 110, 140));
    s.toggleAll();
    expect(s.isFrozen("before") || s.isFrozen("after")).toBe(false);
    expect(s.view("before")!.time).toBe(3);
  });

  it("a held side keeps the settings from when it was held", () => {
    const s = new FreezeStore();
    s.receive(pair(1, 150, 200, 5));
    s.toggle("after");
    s.receive(pair(2, 150, 224, 7));
    expect(s.view("after")!.settings.semitones).toBe(5);
    expect(s.view("before")!.settings.semitones).toBe(7);
  });

  it("keeps the last value faintly while unvoiced", () => {
    const s = new FreezeStore();
    for (let i = 0; i < 25; i++) s.receive(pair(i, 150, 200));
    for (let i = 25; i < 50; i++) {
      s.receive({ ...pair(i, 0, 0), before: { ...frame(0, 0), voiced: false, f0: null }, after: { ...frame(0, 0), voiced: false, f0: null } });
    }
    expect(s.view("before")!.summary!.f0).toBe(150);
    expect(s.view("before")!.summary!.stale).toBe(true);
  });
});
