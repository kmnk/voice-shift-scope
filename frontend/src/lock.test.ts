import { describe, expect, it } from "vitest";
import { LOCK_GUARDED_EVENTS, blocksWhileLocked } from "./lock";

describe("blocksWhileLocked", () => {
  it("stops every key without Ctrl, Alt, or Meta, also on the lock button", () => {
    for (const onLockButton of [false, true]) {
      expect(blocksWhileLocked({ type: "keydown", onLockButton })).toBe(true);
      expect(blocksWhileLocked({ type: "keydown", ctrlKey: true, onLockButton })).toBe(false);
      expect(blocksWhileLocked({ type: "keydown", altKey: true, onLockButton })).toBe(false);
      expect(blocksWhileLocked({ type: "keydown", metaKey: true, onLockButton })).toBe(false);
    }
  });
  it("stops clicks, inputs, changes, and wheels except on the lock button", () => {
    for (const type of ["click", "dblclick", "auxclick", "input", "change", "wheel"]) {
      expect(blocksWhileLocked({ type, onLockButton: false })).toBe(true);
      expect(blocksWhileLocked({ type, onLockButton: true })).toBe(false);
    }
  });
  it("lets other events through, such as the pointer moves that show the cursor on the plots", () => {
    for (const type of ["pointermove", "pointerleave", "keyup", "resize"]) expect(blocksWhileLocked({ type, onLockButton: false })).toBe(false);
  });
  it("lists every event type it stops for main.ts to listen to", () => {
    expect([...LOCK_GUARDED_EVENTS].sort()).toEqual(["auxclick", "change", "click", "dblclick", "input", "keydown", "wheel"]);
  });
});
