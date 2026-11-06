// The lock (L1–L8 in docs/criteria.md): while locked, nothing but the lock button changes the state.
// Real mouse use and focus are stopped with the inert attribute (main.ts). This file decides which events main.ts stops
// on window in the capture phase: keys, and the clicks, inputs, changes, and wheels sent by code (inert does not stop click()).

/// Events main.ts listens to on window in the capture phase while locked
export const LOCK_GUARDED_EVENTS = ["keydown", "click", "dblclick", "auxclick", "input", "change", "wheel"] as const;

export interface LockedEvent {
  type: string;
  ctrlKey?: boolean;
  altKey?: boolean;
  metaKey?: boolean;
  /// Whether the event target is the lock button or inside it
  onLockButton: boolean;
}

/// Whether an event is stopped while locked.
/// Keys: all keys without Ctrl, Alt, or Meta (so Space and Enter do not press a focused control and Tab does not move focus).
/// Keys with them pass, because the app uses none and window keys such as Alt+F4 must keep working.
/// Mouse and value events: everything except on the lock button
export function blocksWhileLocked(e: LockedEvent): boolean {
  if (e.type === "keydown") return !(e.ctrlKey || e.altKey || e.metaKey);
  if ((LOCK_GUARDED_EVENTS as readonly string[]).includes(e.type)) return !e.onLockButton;
  return false;
}
