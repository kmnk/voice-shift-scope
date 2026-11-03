// Entry point of the screen. Calls Tauri commands, and receives analysis results over a channel and draws them.

import { Channel, invoke } from "@tauri-apps/api/core";
import { applyStatic, hasKey, lang, setLang, t, type Lang } from "./i18n";
import { Cursor } from "./cursor";
import { icon, setContent, type IconName } from "./icons";
import { composePng, fileName } from "./png";
import { Spectrogram, drawSpectrum, renderTable } from "./render";
import { STEP, fillRange, formatShift, formantToSlider, normalize, parseShift, sliderToFormant, type ShiftTarget } from "./shift";
import { runSelftest } from "./selftest";
import { TargetOverlay } from "./target";
import { FreezeStore } from "./store";
import type { Devices, Pair, Side } from "./types";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

type View = "full" | "compact";

const store = new FreezeStore();
const grams = { before: new Spectrogram($("before-gram")), after: new Spectrogram($("after-gram")) };
/// Overlay of the held side. When only one side is held, the held side's F1–F4 are drawn as dashed lines on the live side's plot
const targets = { before: new TargetOverlay($("before-target")), after: new TargetOverlay($("after-target")) };
let pending: Pair[] = [];
/// Vertical (frequency) range of the two plots and the spectrum. Uses the larger of the original and shifted ceilings
let axisMax = 5500;
let afterCeiling = 5500;
let devices: Devices | null = null;
let view: View = "full";
let lastStatus: Status | null = null;
/// Whether it has ever started (to remove the guide shown before starting; after stopping, the last plots stay and the guide is not shown)
let everStarted = false;
const cursors = [
  new Cursor($("before-cursor"), "gram", () => axisMax),
  new Cursor($("after-cursor"), "gram", () => axisMax),
  new Cursor($("spectrum-cursor"), "spectrum", () => axisMax),
];

/// Writes the filled range of a slider to the CSS --from and --to (style.css colours the track).
/// From the centre (the default value) to the thumb for pitch and formant, and from the left end to the thumb for volume
function paintRange(r: HTMLInputElement): void {
  const min = Number(r.min);
  const max = Number(r.max);
  const value = Number(r.value);
  let [from, to] = fillRange(min, max, value);
  if (r.id === "volume") [from, to] = [0, max > min ? ((value - min) / (max - min)) * 100 : 0];
  r.style.setProperty("--from", `${from}%`);
  r.style.setProperty("--to", `${to}%`);
}

/// The current pitch (semitones) and formant (factor). These, not the field text, are the source (the fields contain signs and minus signs)
const shift: Record<ShiftTarget, number> = { pitch: 0, formant: 1 };

/// Shows the current values in the value fields (always the same digits) and the sliders (log2 position for formant)
function showShift(target: ShiftTarget): void {
  const v = shift[target];
  const field = $<HTMLInputElement>(`${target}-num`);
  field.value = formatShift(target, v);
  // Values the tests read without depending on language or format
  field.dataset.value = String(v);
  const slider = $<HTMLInputElement>(target);
  slider.value = String(target === "pitch" ? v : formantToSlider(v));
  paintRange(slider);
}

/// Clamps and rounds the pitch or formant value to its range, shows it in the field and slider, and sends it to the shifter
function setShift(target: ShiftTarget, value: number): void {
  shift[target] = normalize(target, value);
  showShift(target);
  void sendShift();
}

/// Steps up or down by the small step (the large step with Shift)
function stepShift(target: ShiftTarget, dir: number, coarse: boolean): void {
  setShift(target, shift[target] + dir * (coarse ? STEP[target].coarse : STEP[target].fine));
}

interface Status {
  running: boolean;
  errors: string[];
  inputXruns: number;
  outputXruns: number;
  monitorXruns: number;
  lastInputXrunSeconds: number;
}

/// The status message on the bar (one place). Its kind (data-kind) changes the look and lets tests read it without depending on language.
/// Errors and dropouts (error, status) get a warning icon. Text that does not fit on the bar is cut with an ellipsis, so the full text goes in the tooltip (title).
let messageTimer: number | undefined;
function message(text: string, kind = "error"): void {
  const m = $("message");
  if (text) setContent(m, kind === "saved" ? null : "warn", text);
  else m.replaceChildren();
  m.title = text;
  m.dataset.kind = text ? kind : "";
  // "Saved" disappears after a few seconds (errors and dropouts stay while they continue)
  window.clearTimeout(messageTimer);
  if (kind === "saved") messageTimer = window.setTimeout(() => message(""), 5000);
}

/// In the compact view, when the content height of the left column changes (for example when a narrower window wraps text), fits the window height again.
/// Rust corrects for the window frame. To avoid correcting forever, at most 4 times per switch.
let compactFixes = 0;
function fitCompactWindow(): void {
  if (view !== "compact" || compactFixes >= 4) return;
  const wanted = compactContentHeight();
  if (Math.abs(wanted - window.innerHeight) <= 2) return;
  compactFixes++;
  void call("set_view", { view: "compact", contentHeight: wanted });
}

/// Window height in the compact view: the content heights of the bar and the left column, plus the gap between them and the body's top and bottom padding
function compactContentHeight(): number {
  const controls = document.querySelector(".controls") as HTMLElement;
  const bar = document.querySelector(".bar") as HTMLElement;
  const style = getComputedStyle(document.body);
  return Math.ceil(bar.offsetHeight + parseFloat(style.rowGap) + controls.scrollHeight + parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) + 2);
}

/// Show a command error in the current language: look it up by its name (filling in its values), or show the text as received.
function errorText(e: unknown): string {
  const err = e as { code?: string; message?: string; params?: Record<string, string> };
  const key = `err.${err.code ?? ""}`;
  if (err.code && hasKey(key)) return t(key, err.params ?? {});
  return err.message ?? String(e);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    message(errorText(e));
    return undefined;
  }
}

function fillSelect(select: HTMLSelectElement, list: { id: string; name: string }[], selected: string | null): void {
  select.innerHTML =
    `<option value="">${t("unselected")}</option>` + list.map((d) => `<option value="${d.id}">${d.name}</option>`).join("");
  select.value = selected ?? "";
}

function showDevices(d: Devices): void {
  devices = d;
  fillSelect($("input"), d.inputs, d.selectedInput);
  fillSelect($("monitor"), d.monitors, d.selectedMonitor);
  $("monitor-on").setAttribute("aria-pressed", String(d.monitorEnabled));
  ($("volume") as HTMLInputElement).value = String(d.monitorVolume);
  paintRange($("volume"));
  fillSelect($("output"), d.outputs, d.selectedOutput);
  // VB-CABLE is one of the output devices and is not assumed, so whether it is installed is not shown (only a mark for tests is kept)
  $("cable").dataset.found = String(!!d.cable);
  // The guide before starting: shown in the plot, only until the first start, telling what to do to start
  if (d.running) everStarted = true;
  $("plot-guide").textContent = everStarted ? "" : t(d.selectedOutput ? "guide.start" : "guide.output");
  // The output and the monitor get the same audio (the monitor is for checking what is being sent; the user made it one setting on 2026-10-18).
  // One button that shows the current value
  const route = $("route");
  route.dataset.source = d.outputSource;
  setContent(route, "route", t(d.outputSource === "shifted" ? "route.shifted" : "route.original"));
  // Start and stop share one button. It cannot start while no output is chosen
  const run = $<HTMLButtonElement>("run");
  run.dataset.running = String(d.running);
  setContent(run, d.running ? "stop" : "start", t(d.running ? "stop" : "start"));
  run.disabled = !d.running && !d.selectedOutput;
  ($("output") as HTMLSelectElement).disabled = d.running;
  ($("input") as HTMLSelectElement).disabled = d.running;
}

function updateFreezeButtons(): void {
  for (const side of ["before", "after"] as const) {
    const frozen = store.isFrozen(side);
    $(`freeze-${side}`).setAttribute("aria-pressed", String(frozen));
    // A held plot is marked with its border and label (.panel[data-held] in style.css)
    $(`${side}-panel`).dataset.held = String(frozen);
    const v = store.view(side);
    const state = $(`${side}-state`);
    state.textContent = frozen && v ? t("state.held", { t: v.time.toFixed(2) }) : "";
    state.classList.toggle("frozen", frozen);
    state.dataset.frozen = String(frozen);
  }
  const all = store.isFrozen("before") && store.isFrozen("after");
  $("freeze-all").setAttribute("aria-pressed", String(all));
  ($("save") as HTMLButtonElement).disabled = !store.anyFrozen();
}

function toggle(side: Side | "all"): void {
  if (side === "all") store.toggleAll();
  else store.toggle(side);
  updateFreezeButtons();
  draw();
}

function draw(): void {
  if (view === "compact") return;
  const before = store.view("before");
  const after = store.view("after");
  drawSpectrum($("spectrum"), before, after, axisMax);
  renderTable($("numbers"), before, after);
  // The values at the right edge are the same 250 ms medians as the table (a held side keeps the values from when it was held).
  // When only one side is held, the held side's values are overlaid on the live side's plot as targets (not when both are held or both are live)
  const formants = (v: typeof before) => [0, 1, 2, 3].map((i) => v?.summary?.formants[i]?.frequency ?? null);
  for (const [side, v, other] of [["before", before, after], ["after", after, before]] as const) {
    const otherSide = side === "before" ? "after" : "before";
    const overlay = !store.isFrozen(side) && store.isFrozen(otherSide) && other !== null;
    const goal = overlay ? formants(other) : [];
    grams[side].setValues(formants(v), v?.summary?.stale ?? false, goal, overlay ? (other?.summary?.stale ?? false) : false);
    targets[side].set(goal, axisMax);
    const legend = $(`${side}-target-legend`);
    legend.hidden = !overlay;
    (legend.querySelector(".label") as HTMLElement).textContent = overlay && other ? t("overlay.legend", { t: other.time.toFixed(2), side: t(otherSide === "before" ? "original" : "shifted") }) : "";
  }
  const differ = $("times-differ");
  if (store.timesDiffer()) setContent(differ, "warn", t("times.differ"));
  else differ.replaceChildren();
  differ.dataset.differ = String(store.timesDiffer());
  $("ceiling-note").textContent = t("ceiling.note", { hz: afterCeiling.toFixed(0) });
}

/// When the language changes, also rewrites dynamically written text.
function refreshTexts(): void {
  applyStatic();
  // The view switch is an icon-only button. Its name is in the tooltip and for screen readers
  const viewToggle = $("view-toggle");
  setContent(viewToggle, view === "full" ? "compact" : "full", "");
  viewToggle.title = t(view === "full" ? "view.compact.title" : "view.full.title");
  viewToggle.setAttribute("aria-label", viewToggle.title);
  if (devices) showDevices(devices);
  updateFreezeButtons();
  if (lastStatus) showStatus(lastStatus);
  draw();
}

function applyView(v: View): void {
  view = v;
  // The settings panel exists only in the full view (the analysis stops in the compact view)
  closeSettings();
  document.body.classList.toggle("compact", v === "compact");
  refreshTexts();
  if (v === "full") {
    grams.before.prepare(axisMax);
    grams.after.prepare(axisMax);
  }
}

// Counts for tests (number of display updates, and the delay from receiving to drawing)
const stats = { frames: 0, since: performance.now(), lastLagMs: 0, received: 0 };

function loop(now: number): void {
  if (pending.length > 0) {
    const batch = pending;
    pending = [];
    for (const pair of batch) {
      store.receive(pair);
      axisMax = Math.max(pair.settings.ceiling, pair.settings.afterCeiling);
      afterCeiling = pair.settings.afterCeiling;
      if (view === "full") {
        for (const side of ["before", "after"] as const) {
          grams[side].prepare(axisMax);
          if (!store.isFrozen(side)) grams[side].push(pair[side]);
        }
      }
    }
    const arrived = (batch[batch.length - 1] as Pair & { arrivedAt: number }).arrivedAt;
    stats.lastLagMs = now - arrived;
    draw();
    stats.frames++;
  }
  if (now - stats.since >= 1000) {
    $("test-stats").textContent = JSON.stringify({
      drawsPerSecond: (stats.frames * 1000) / (now - stats.since),
      receiveToDrawMs: stats.lastLagMs,
      received: stats.received,
      frozen: { before: store.isFrozen("before"), after: store.isFrozen("after") },
    });
    stats.frames = 0;
    stats.since = now;
  }
  requestAnimationFrame(loop);
}

async function start(): Promise<void> {
  const channel = new Channel<Pair>();
  channel.onmessage = (pair) => {
    (pair as Pair & { arrivedAt: number }).arrivedAt = performance.now();
    stats.received++;
    pending.push(pair);
  };
  const d = await call<Devices>("start", { onAnalysis: channel });
  if (d) {
    message("");
    showDevices(d);
  }
}

async function sendShift(): Promise<void> {
  await call("change_shift", {
    semitones: shift.pitch,
    formant: shift.formant,
  });
}

async function sendAnalysis(): Promise<void> {
  await call("change_analysis", {
    ceiling: Number(($("ceiling") as HTMLInputElement).value),
    threshold: Number(($("threshold") as HTMLInputElement).value),
    relativeLimit: Number(($("relative-limit") as HTMLInputElement).value),
    lpcOrder: 10,
  });
}

/// Pitch and formant controls: the slider, typing in the value field, ↑↓ in the field, the wheel, and the − and + buttons
function wireShift(target: ShiftTarget): void {
  const slider = $<HTMLInputElement>(target);
  const field = $<HTMLInputElement>(`${target}-num`);
  slider.addEventListener("input", () => {
    const position = Number(slider.value);
    setShift(target, target === "pitch" ? position : sliderToFormant(position));
  });
  // Clicking a field selects its text. Enter or losing focus turns it into a value; unreadable text goes back to the previous value
  field.addEventListener("focus", () => field.select());
  const commit = () => {
    const v = parseShift(field.value);
    if (v === null) showShift(target);
    else setShift(target, v);
  };
  field.addEventListener("change", commit);
  field.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
      field.select();
    } else if (e.key === "Escape") {
      e.preventDefault();
      showShift(target);
      field.select();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      stepShift(target, e.key === "ArrowUp" ? 1 : -1, e.shiftKey);
      field.select();
    }
  });
  // Wheel: up increases, down decreases. With Shift, Chromium may turn it into horizontal scrolling (deltaX), so both are read.
  // A touchpad sends many fine events and would move too fast, so they are thinned to one step per 100 ms
  let lastWheel = 0;
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const delta = e.deltaY !== 0 ? e.deltaY : e.deltaX;
    if (delta === 0 || e.timeStamp - lastWheel < 100) return;
    lastWheel = e.timeStamp;
    stepShift(target, delta < 0 ? 1 : -1, e.shiftKey);
  };
  field.addEventListener("wheel", onWheel, { passive: false });
  slider.addEventListener("wheel", onWheel, { passive: false });
}

async function save(): Promise<void> {
  if (!store.anyFrozen()) return;
  const now = new Date();
  const bytes = await composePng(
    [$("before-gram") as HTMLCanvasElement, $("after-gram") as HTMLCanvasElement],
    [$("before-target") as HTMLCanvasElement, $("after-target") as HTMLCanvasElement],
    $("spectrum") as HTMLCanvasElement,
    store.view("before"),
    store.view("after"),
    now,
  );
  const path = await call<string | null>("save_png", { bytes: Array.from(bytes), fileName: fileName(now) });
  if (path) message(t("saved", { path }), "saved");
}

function showStatus(s: Status): void {
  lastStatus = s;
  const parts = [...new Set(s.errors)];
  const xruns = [
    s.inputXruns > 0 ? t("dropouts.in", { n: s.inputXruns, s: s.lastInputXrunSeconds.toFixed(1) }) : "",
    s.outputXruns > 0 ? t("dropouts.out", { n: s.outputXruns }) : "",
    s.monitorXruns > 0 ? t("dropouts.mon", { n: s.monitorXruns }) : "",
  ].filter(Boolean);
  if (xruns.length > 0) parts.push(t("dropouts", { parts: xruns.join(", ") }));
  if (parts.length > 0) message(parts.join(" / "), "status");
}

function wire(): void {
  $("run").addEventListener("click", async () => {
    if (!devices?.running) {
      await start();
      return;
    }
    const d = await call<Devices>("stop");
    if (d) showDevices(d);
  });
  $("output").addEventListener("change", async (e) => {
    const id = (e.target as HTMLSelectElement).value;
    if (!id) return;
    const d = await call<Devices>("select_output", { id });
    if (d) showDevices(d);
    else if (devices) showDevices(devices);
  });
  $("input").addEventListener("change", async (e) => {
    const id = (e.target as HTMLSelectElement).value;
    if (!id) return;
    const d = await call<Devices>("select_input", { id });
    if (d) showDevices(d);
    else if (devices) showDevices(devices);
  });
  // The monitor ON/OFF is a toggle button. Pressing it sends the opposite value; if refused, the display goes back to the current value
  const monitorChange = async (enabled: boolean) => {
    const id = ($("monitor") as HTMLSelectElement).value || null;
    const d = await call<Devices>("set_monitor", { enabled, id });
    if (d) showDevices(d);
    else if (devices) showDevices(devices);
  };
  const monitorPressed = () => $("monitor-on").getAttribute("aria-pressed") === "true";
  $("monitor").addEventListener("change", () => void monitorChange(monitorPressed()));
  $("monitor-on").addEventListener("click", () => void monitorChange(!monitorPressed()));
  $("volume").addEventListener("input", (e) => {
    paintRange(e.target as HTMLInputElement);
    void call("set_monitor_volume", { volume: Number((e.target as HTMLInputElement).value) });
  });
  wireShift("pitch");
  wireShift("formant");
  // The source button switches to the other side when pressed
  $("route").addEventListener("click", async () => {
    if (!devices) return;
    const source: Devices["outputSource"] = devices.outputSource === "shifted" ? "original" : "shifted";
    devices.outputSource = source;
    devices.monitorSource = source;
    showDevices(devices);
    const d = await call<Devices>("set_routing", { output: source, monitor: source });
    if (d) showDevices(d);
  });
  // The − and + on each side of the value field (the large step with Shift), and the reset button
  const defaults: Record<ShiftTarget, number> = { pitch: 0, formant: 1 };
  document.querySelectorAll<HTMLButtonElement>("button.step").forEach((b) =>
    b.addEventListener("click", (e) => stepShift(b.dataset.target as ShiftTarget, Number(b.dataset.dir), e.shiftKey)),
  );
  document.querySelectorAll<HTMLButtonElement>("button.reset").forEach((b) =>
    b.addEventListener("click", () => setShift(b.dataset.target as ShiftTarget, defaults[b.dataset.target as ShiftTarget])),
  );
  $("ceiling").addEventListener("change", () => void sendAnalysis());
  $("threshold").addEventListener("change", () => void sendAnalysis());
  $("relative-limit").addEventListener("change", () => void sendAnalysis());
  $("freeze-all").addEventListener("click", () => toggle("all"));
  $("freeze-before").addEventListener("click", () => toggle("before"));
  $("freeze-after").addEventListener("click", () => toggle("after"));
  $("save").addEventListener("click", () => void save());
  $("lang-toggle").addEventListener("click", () => {
    const next: Lang = lang() === "en" ? "ja" : "en";
    setLang(next);
    refreshTexts();
    void call("set_language", { language: next });
  });
  $("view-toggle").addEventListener("click", () => {
    const next: View = view === "full" ? "compact" : "full";
    // Do not carry over the previous status message (such as "Saved")
    message("");
    compactFixes = 0;
    applyView(next);
    void call("set_view", { view: next, contentHeight: next === "compact" ? compactContentHeight() : null });
  });
  const observer = new ResizeObserver(() => fitCompactWindow());
  observer.observe(document.querySelector(".controls") as HTMLElement);
  observer.observe(document.querySelector(".bar") as HTMLElement);
  wireSettings();
  window.addEventListener("resize", () => {
    if (view === "compact") {
      fitCompactWindow();
      return;
    }
    grams.before.prepare(axisMax);
    grams.after.prepare(axisMax);
    draw();
    targets.before.draw();
    targets.after.draw();
    for (const c of cursors) c.draw();
  });
  wireShortcuts();
  // Show audio errors, such as a device disappearing while running
  setInterval(async () => {
    const s = await invoke<Status>("status").catch(() => null);
    if (s) showStatus(s);
  }, 1000);
}

/// The settings panel (analysis settings). Opened with the settings button on the bar. Closed with Esc, clicking outside, or pressing the button again.
/// Closing is handled here rather than left to the browser's light dismiss (so the keys and clicks the tests send behave the same)
function settingsOpen(): boolean {
  return $("settings").matches(":popover-open");
}
function openSettings(): void {
  const panel = $("settings");
  if (settingsOpen()) return;
  panel.showPopover();
  $("settings-open").setAttribute("aria-expanded", "true");
  $<HTMLInputElement>("ceiling").focus();
}
function closeSettings(): void {
  const panel = $("settings");
  if (!settingsOpen()) return;
  const focusWasInside = panel.contains(document.activeElement);
  panel.hidePopover();
  $("settings-open").setAttribute("aria-expanded", "false");
  if (focusWasInside) $("settings-open").focus();
}
function wireSettings(): void {
  $("settings-open").addEventListener("click", () => (settingsOpen() ? closeSettings() : openSettings()));
  document.addEventListener("pointerdown", (e) => {
    const target = e.target as Node;
    if (settingsOpen() && !$("settings").contains(target) && !$("settings-open").contains(target)) closeSettings();
  });
}

/// Opens and closes the keyboard shortcuts. On close, focus returns to where it was before opening
let focusBeforeShortcuts: HTMLElement | null = null;
function openShortcuts(): void {
  const dialog = $<HTMLDialogElement>("shortcuts");
  if (dialog.open) return;
  focusBeforeShortcuts = document.activeElement as HTMLElement | null;
  dialog.showModal();
  $("shortcuts-close").focus();
}
function closeShortcuts(): void {
  const dialog = $<HTMLDialogElement>("shortcuts");
  if (dialog.open) dialog.close();
}

/// Keys. They work only in the full view, and not while typing in a number field, select, or slider, or with Ctrl, Alt, or Meta.
/// While the shortcut list is open, only the keys that close it (? and Esc) work.
function wireShortcuts(): void {
  const dialog = $<HTMLDialogElement>("shortcuts");
  $("shortcuts-open").addEventListener("click", openShortcuts);
  $("shortcuts-close").addEventListener("click", closeShortcuts);
  // Clicking the dark area behind (the dialog element itself) closes it
  dialog.addEventListener("click", (e) => {
    if (e.target === dialog) closeShortcuts();
  });
  dialog.addEventListener("close", () => {
    focusBeforeShortcuts?.focus();
    focusBeforeShortcuts = null;
  });
  document.addEventListener("keydown", (e) => {
    if (e.ctrlKey || e.altKey || e.metaKey) return;
    if (e.key === "Escape" && settingsOpen()) {
      e.preventDefault();
      closeSettings();
      return;
    }
    if (dialog.open) {
      if (e.key === "?" || e.key === "Escape") {
        e.preventDefault();
        closeShortcuts();
      }
      return;
    }
    const target = e.target as HTMLElement;
    if (view !== "full" || ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName)) return;
    const shiftValue = (name: ShiftTarget, _fine: number, _coarse: number, sign: number) => {
      e.preventDefault();
      stepShift(name, sign, e.shiftKey);
    };
    switch (e.key) {
      case " ":
      case "0":
        e.preventDefault();
        toggle("all");
        break;
      case "1":
        toggle("before");
        break;
      case "2":
        toggle("after");
        break;
      case "s":
      case "S":
        if (!$<HTMLButtonElement>("save").disabled) void save();
        break;
      case "ArrowUp":
        shiftValue("pitch", 0.1, 1, 1);
        break;
      case "ArrowDown":
        shiftValue("pitch", 0.1, 1, -1);
        break;
      case "ArrowRight":
        shiftValue("formant", 0.01, 0.05, 1);
        break;
      case "ArrowLeft":
        shiftValue("formant", 0.01, 0.05, -1);
        break;
      case "?":
        e.preventDefault();
        openShortcuts();
        break;
    }
  });
}

async function boot(): Promise<void> {
  // Place the fixed icons (they do not change with the language)
  document.querySelectorAll<HTMLElement>("[data-icon]").forEach((el) => {
    if (!el.querySelector(":scope > svg.icon")) el.prepend(icon(el.dataset.icon as IconName));
  });
  showShift("pitch");
  showShift("formant");
  paintRange($("volume"));
  wire();
  const prefs = await invoke<{ language: Lang; view: View }>("get_ui_prefs").catch(() => ({ language: "en" as Lang, view: "full" as View }));
  setLang(prefs.language);
  applyView(prefs.view);
  // When started in the compact view, also fit the window to the content height
  if (prefs.view === "compact") void call("set_view", { view: "compact", contentHeight: compactContentHeight() });
  const d = await call<Devices>("list_devices");
  if (d) {
    showDevices(d);
    // Show the remembered pitch and formant in the fields and sliders (they were given to the shifter at start)
    shift.pitch = normalize("pitch", d.shift.semitones);
    shift.formant = normalize("formant", d.shift.formant);
    showShift("pitch");
    showShift("formant");
    // Put the remembered analysis settings into the fields
    ($("ceiling") as HTMLInputElement).value = String(d.analysis.ceiling);
    ($("threshold") as HTMLInputElement).value = String(d.analysis.threshold);
    ($("relative-limit") as HTMLInputElement).value = String(d.analysis.relativeLimit);
    document.querySelectorAll<HTMLInputElement>('input[type="range"]').forEach(paintRange);
  }
  requestAnimationFrame(loop);
  const dir = await invoke<string | null>("selftest_config").catch(() => null);
  if (dir) void runSelftest(dir);
}

void boot();
