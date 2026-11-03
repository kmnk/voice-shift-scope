// End-to-end UI test (runs only when started with `--ui-selftest <dir>`).
// Performs the same operations as a user (pressing buttons, entering values) on the DOM in order, and judges by reading what appears on screen.
// To avoid depending on language, state is read from data-* marks rather than text. Results go to <dir>/ui-report.json, and the app exits.

import { invoke } from "@tauri-apps/api/core";
import { lang, t } from "./i18n";
import { formatDb, formatKHz, gramFreqAt, spectrumDbAt, spectrumFreqAt, spectrumLayout } from "./render";
import { formantToSlider } from "./shift";
import { PNG_HEADER, composePng } from "./png";
import { targetLineYs } from "./target";

type Result = { criterion: string; result: "PASS" | "FAIL"; detail: unknown };

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const q = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;
const click = (testid: string) => q<HTMLButtonElement>(`[data-testid="${testid}"]`).click();
const disabled = (testid: string) => q<HTMLButtonElement>(`[data-testid="${testid}"]`).disabled;
const frozen = (side: string) => q(`#${side}-state`).dataset.frozen === "true";
const stateText = (side: string) => q(`#${side}-state`).textContent ?? "";
const near = (v: number, target: number, pct: number) => Math.abs(v / target - 1) * 100 <= pct;
const pressed = (testid: string) => q(`[data-testid="${testid}"]`).getAttribute("aria-pressed") === "true";

// Rust writes a shot of the window at the time each acceptance item is checked to <dir>/shots/<name>.png (Windows only).
// Failing to take a shot does not count in the test results.
async function shot(name: string): Promise<void> {
  // Right after a DOM change, it is not painted yet. Wait for two repaints before taking the shot
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  try {
    await invoke<string>("selftest_shot", { name });
  } catch {
    // Failing to take a shot is not a test failure
  }
}

/// Changes the inner window size and waits until the screen has that size (Rust's selftest_resize, which works only through the test entry point).
async function resizeWindow(width: number, height: number): Promise<{ ok: boolean; innerWidth: number; innerHeight: number; error?: string; firstTry?: { innerWidth: number; innerHeight: number } }> {
  try {
    await invoke("selftest_resize", { width, height });
  } catch (e) {
    return { ok: false, innerWidth: window.innerWidth, innerHeight: window.innerHeight, error: String(e) };
  }
  const fits = () => Math.abs(window.innerWidth - width) <= 2 && Math.abs(window.innerHeight - height) <= 2;
  let ok = await waitFor(fits, 3000);
  // On OSes where the window size includes the frame and title bar (on macOS the height was 32 px short), ask once more with the difference added so the inside gets the requested size
  const first = { innerWidth: window.innerWidth, innerHeight: window.innerHeight };
  if (!ok) {
    try {
      await invoke("selftest_resize", { width: width + (width - first.innerWidth), height: height + (height - first.innerHeight) });
    } catch (e) {
      return { ok: false, ...first, error: String(e) };
    }
    ok = await waitFor(fits, 3000);
  }
  // Wait for the repaint after the size change
  await sleep(800);
  return { ok, innerWidth: window.innerWidth, innerHeight: window.innerHeight, ...(ok && (first.innerWidth !== width || first.innerHeight !== height) ? { firstTry: first } : {}) };
}

/// The current pitch or formant value (the field text contains signs and minus signs, so read the data-value main.ts writes)
const shiftValue = (target: "pitch" | "formant") => Number(q<HTMLInputElement>(`#${target}-num`).dataset.value);

/// Look and controls of the pitch and formant fields (U6, U7, U8). Works without VB-CABLE
async function checkShiftControls(record: (c: string, pass: boolean, detail: unknown) => Promise<void>): Promise<void> {
  // U6: the spectrum title is text only, with a legend of line samples
  const caption = q(".spectrum-panel .caption-title").textContent ?? "";
  const lines = [...document.querySelectorAll<SVGLineElement>('[data-testid="spectrum-legend"] line')];
  const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const strokes = lines.map((l) => getComputedStyle(l).stroke);
  const probe = document.createElement("span");
  document.body.append(probe);
  const rgb = (name: string) => {
    probe.style.color = css(name);
    return getComputedStyle(probe).color;
  };
  const expected = [rgb("--before"), rgb("--accent")];
  probe.remove();
  const dashed = lines.map((l) => l.getAttribute("stroke-dasharray") ?? "");
  await record("U6-spectrum-legend", lines.length === 2 && strokes[0] === expected[0] && strokes[1] === expected[1] && dashed[0] !== "" && dashed[1] === "" && !/gray|dashed|灰|破線/.test(caption), {
    caption,
    strokes,
    expected,
    dashed,
  });

  const text = (t: "pitch" | "formant") => q<HTMLInputElement>(`#${t}-num`).value;
  const slider = (t: "pitch" | "formant") => q<HTMLInputElement>(`#${t}`);
  const reset = (t: "pitch" | "formant") => q<HTMLButtonElement>(`button.reset[data-target="${t}"]`).click();
  const wheel = (el: Element, deltaY: number, shiftKey = false) => el.dispatchEvent(new WheelEvent("wheel", { deltaY, shiftKey, bubbles: true, cancelable: true }));
  const typeIn = (t: "pitch" | "formant", value: string) => {
    const f = q<HTMLInputElement>(`#${t}-num`);
    f.value = value;
    f.dispatchEvent(new Event("change"));
  };
  reset("pitch");
  reset("formant");

  // U7: whatever operation changes them, pitch shows as +5.0 or 0.0 and formant as 1.00
  const s = slider("pitch");
  s.value = "5";
  s.dispatchEvent(new Event("input"));
  const bySlider = text("pitch");
  reset("pitch");
  const byReset = text("pitch");
  typeIn("pitch", "+5");
  const byTyping = text("pitch");
  const sentValue = shiftValue("pitch");
  typeIn("pitch", "abc");
  const afterBad = text("pitch");
  typeIn("pitch", "−3.2");
  const negative = text("pitch");
  reset("pitch");
  const formantText = text("formant");
  await record("U7-digits", bySlider === "+5.0" && byReset === "0.0" && byTyping === "+5.0" && sentValue === 5 && afterBad === "+5.0" && negative === "−3.2" && formantText === "1.00", {
    bySlider,
    byReset,
    byTyping,
    sentValue,
    afterBad,
    negative,
    formantText,
  });

  // U8: there is no row of step buttons; − and + (the large step with Shift) and the wheel move the values. Formant ×1.00 is at the slider's centre
  const oldSteps = document.querySelectorAll("button[data-step]").length;
  const click = (testid: string, shiftKey = false) =>
    q(`[data-testid="${testid}"]`).dispatchEvent(new MouseEvent("click", { shiftKey, bubbles: true, cancelable: true }));
  click("pitch-up");
  const pUp = shiftValue("pitch");
  click("pitch-up", true);
  const pUpCoarse = shiftValue("pitch");
  click("pitch-down", true);
  click("pitch-down");
  const pBack = shiftValue("pitch");
  click("formant-up");
  const fUp = shiftValue("formant");
  click("formant-up", true);
  const fUpCoarse = shiftValue("formant");
  click("formant-down", true);
  click("formant-down");
  const fBack = shiftValue("formant");
  const field = q<HTMLInputElement>("#pitch-num");
  wheel(field, -100);
  const wUp = shiftValue("pitch");
  await sleep(150);
  wheel(field, -100, true);
  const wUpCoarse = shiftValue("pitch");
  await sleep(150);
  wheel(slider("formant"), -100);
  const fWheel = shiftValue("formant");
  reset("pitch");
  reset("formant");
  const centered = Number(slider("formant").value) === formantToSlider(1) && Number(slider("formant").value) === 0;
  const f = slider("formant");
  f.value = f.min;
  f.dispatchEvent(new Event("input"));
  const atMin = text("formant");
  f.value = f.max;
  f.dispatchEvent(new Event("input"));
  const atMax = text("formant");
  reset("formant");
  const close = (a: number, b: number) => Math.abs(a - b) < 1e-6;
  await record(
    "U8-controls",
    oldSteps === 0 &&
      close(pUp, 0.1) && close(pUpCoarse, 1.1) && close(pBack, 0) &&
      close(fUp, 1.01) && close(fUpCoarse, 1.06) && close(fBack, 1) &&
      close(wUp, 0.1) && close(wUpCoarse, 1.1) && close(fWheel, 1.01) &&
      centered && atMin === "0.50" && atMax === "2.00",
    { oldSteps, pUp, pUpCoarse, pBack, fUp, fUpCoarse, fBack, wUp, wUpCoarse, fWheel, centered, atMin, atMax },
  );
}

/// Whether the values table fits in its section (no horizontal or vertical overflow) and the two spectrograms are the same size (U4)
function tableFits(): { lang: string; fits: boolean; table: number[]; panel: number[]; grams: number[][] } {
  const t = q("#numbers").getBoundingClientRect();
  const p = q(".table-panel").getBoundingClientRect();
  const g = ["before", "after"].map((side) => {
    const r = q(`#${side}-gram`).getBoundingClientRect();
    return [Math.round(r.width), Math.round(r.height)];
  });
  const inside = t.right <= p.right + 0.5 && t.bottom <= p.bottom + 0.5 && t.left >= p.left - 0.5;
  const same = g[0][0] === g[1][0] && g[0][1] === g[1][1];
  return { lang: lang(), fits: inside && same, table: [t.width, t.height].map(Math.round), panel: [p.width, p.height].map(Math.round), grams: g };
}

/// Checks tableFits in both English and Japanese (switches the language and back)
async function tableFitsBothLanguages(): Promise<ReturnType<typeof tableFits>[]> {
  const first = tableFits();
  click("lang-toggle");
  await sleep(300);
  const second = tableFits();
  click("lang-toggle");
  await sleep(300);
  return [first, second];
}

/// Overlay (U10, U11). Called with only SHIFTED held, and ends back in the same state
async function checkOverlay(record: (c: string, pass: boolean, detail: unknown) => Promise<void>): Promise<void> {
  await sleep(500);
  const look = () =>
    Object.fromEntries(
      (["before", "after"] as const).map((side) => [
        side,
        {
          targets: q<HTMLCanvasElement>(`#${side}-target`).dataset.targets ?? "",
          values: q<HTMLCanvasElement>(`#${side}-gram`).dataset.values ?? "",
          legend: q(`#${side}-target-legend`).hidden ? "" : (q(`#${side}-target-legend`).textContent ?? ""),
        },
      ]),
    ) as Record<"before" | "after", { targets: string; values: string; legend: string }>;
  const shiftedHeld = look();
  await shot("U10-overlay");
  // Hold only ORIGINAL
  click("freeze-after");
  click("freeze-before");
  await sleep(500);
  const originalHeld = look();
  // Hold both sides
  click("freeze-after");
  await sleep(300);
  const bothHeld = look();
  // Back to only SHIFTED held
  click("freeze-before");
  await sleep(300);
  const restored = look();
  const filled = (s: string) => s.split(",").filter((v) => v !== "").length >= 2;
  const shiftedSide = t("shifted");
  await record(
    "U10-overlay",
    filled(shiftedHeld.before.targets) &&
      shiftedHeld.before.targets === shiftedHeld.after.values &&
      shiftedHeld.after.targets === "" &&
      shiftedHeld.before.legend.includes(shiftedSide) &&
      shiftedHeld.after.legend === "" &&
      filled(originalHeld.after.targets) &&
      originalHeld.after.targets === originalHeld.before.values &&
      originalHeld.before.targets === "" &&
      bothHeld.before.targets === "" &&
      bothHeld.after.targets === "" &&
      bothHeld.before.legend === "" &&
      // Holding again takes the values at that moment, so check that the overlaid values equal the held side's values rather than the first values
      filled(restored.before.targets) &&
      restored.before.targets === restored.after.values,
    { shiftedHeld, originalHeld, bothHeld, restored },
  );

  // U11: in the saved PNG, the pixels at the target heights in the ORIGINAL plot have a dashed line in that formant's colour
  const grams: [HTMLCanvasElement, HTMLCanvasElement] = [q("#before-gram"), q("#after-gram")];
  const targetsCanvases: [HTMLCanvasElement, HTMLCanvasElement] = [q("#before-target"), q("#after-target")];
  const bytes = await composePng(grams, targetsCanvases, q<HTMLCanvasElement>("#spectrum"), null, null, new Date());
  const bitmap = await createImageBitmap(new Blob([bytes.slice().buffer as ArrayBuffer], { type: "image/png" }));
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  const c = canvas.getContext("2d")!;
  c.drawImage(bitmap, 0, 0);
  const dpr = window.devicePixelRatio || 1;
  const gw = grams[0].clientWidth;
  const gh = grams[0].clientHeight;
  const goal = restored.before.targets.split(",").map((v) => (v === "" ? null : Number(v)));
  const axisMax = Number(q<HTMLInputElement>("#ceiling").value) * Math.max(1, shiftValue("formant"));
  const ys = targetLineYs(goal, axisMax, gh);
  const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const hex = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const found = ys.map((y, i) => {
    if (y === null) return null;
    const want = hex(css(`--formant-${i + 1}`));
    // It is dashed, so look at several points across the line's height and accept if any is close to the colour
    for (let x = 60; x < gw - 60; x += 3) {
      const px = c.getImageData(Math.round((10 + x) * dpr), Math.round((PNG_HEADER + y) * dpr), 1, 1).data;
      if (Math.abs(px[0] - want[0]) + Math.abs(px[1] - want[1]) + Math.abs(px[2] - want[2]) < 60) return true;
    }
    return false;
  });
  await record("U11-overlay-png", found.filter((f) => f !== null).length >= 2 && found.every((f) => f !== false), { goal, axisMax, found });
}

function setNumber(id: string, value: number): void {
  const e = q<HTMLInputElement>(`#${id}`);
  e.value = String(value);
  e.dispatchEvent(new Event("change"));
}

function f0Row(): [number, number] {
  const row = [...document.querySelectorAll<HTMLTableRowElement>('[data-testid="numbers"] tr')].find((r) =>
    r.cells[0]?.textContent?.startsWith("F0"),
  );
  return row ? [Number(row.cells[1].textContent), Number(row.cells[2].textContent)] : [NaN, NaN];
}

async function waitFor(cond: () => boolean, ms: number): Promise<boolean> {
  const end = performance.now() + ms;
  while (performance.now() < end) {
    if (cond()) return true;
    await sleep(100);
  }
  return cond();
}

/// Whether the DEVICES section buttons fit on one line and within the section with every possible label (measured in the current language).
/// Swaps the labels temporarily to measure, and restores them afterwards.
function devicesTextsFit(): { lang: string; id: string; text: string; fits: boolean; scrollWidth: number; clientWidth: number; height: number }[] {
  const cases: [string, string[]][] = [
    ["run", [t("start"), t("stop")]],
    ["monitor-on", [t("monitor.on")]],
    ["route", [t("route.shifted"), t("route.original")]],
  ];
  const out = [];
  for (const [id, texts] of cases) {
    const b = q<HTMLButtonElement>(`#${id}`);
    // The text is in the .label span (swap only the span so the icon SVG stays)
    const label = b.querySelector<HTMLElement>(".label") ?? b;
    const keep = label.textContent;
    for (const text of texts) {
      label.textContent = text;
      const rect = b.getBoundingClientRect();
      // Start/stop is on the bar and the rest in the DEVICES section. None goes beyond the right edge of its container
      const groupRight = (b.closest(".group, .bar") as HTMLElement).getBoundingClientRect().right;
      const fits = b.scrollWidth <= b.clientWidth + 1 && rect.right <= groupRight && rect.height <= 25;
      out.push({ lang: lang(), id, text, fits, scrollWidth: b.scrollWidth, clientWidth: b.clientWidth, height: rect.height });
    }
    label.textContent = keep;
  }
  return out;
}

/// The monitor ON/OFF button. Turning it ON without a device is refused and the display goes back to OFF.
/// If there is a device that can be the monitor (other than the output), also checks that ON and OFF switch. Done while stopped, so no device is opened.
async function checkMonitor(record: (c: string, pass: boolean, detail: unknown) => Promise<void>): Promise<void> {
  const button = q<HTMLElement>("#monitor-on");
  const select = q<HTMLSelectElement>("#monitor");
  const group = button.closest(".group") as HTMLElement;
  const isButton = button.tagName === "BUTTON";
  const checkboxes = group.querySelectorAll('input[type="checkbox"]').length;
  const height = button.getBoundingClientRect().height;
  const keep = select.value;
  select.value = "";
  select.dispatchEvent(new Event("change"));
  await sleep(300);
  click("monitor-on");
  await sleep(400);
  const rejectedStaysOff = !pressed("monitor-on");
  const rejectMessage = q("#message").textContent;
  await record("monitor-button", isButton && checkboxes === 0 && height >= 24 && rejectedStaysOff && rejectMessage !== "", {
    isButton,
    checkboxes,
    height,
    rejectedStaysOff,
    rejectMessage,
  });
  const output = q<HTMLSelectElement>("#output").value;
  const candidate = [...select.options].find((o) => o.value !== "" && o.value !== output);
  if (candidate) {
    select.value = candidate.value;
    select.dispatchEvent(new Event("change"));
    await sleep(300);
    click("monitor-on");
    await sleep(400);
    const on = pressed("monitor-on");
    click("monitor-on");
    await sleep(400);
    const off = !pressed("monitor-on");
    await record("monitor-toggle", on && off, { device: candidate.textContent, on, off });
  }
  // Restore the original choice (still OFF)
  select.value = keep;
  select.dispatchEvent(new Event("change"));
  await sleep(300);
  q("#message").textContent = "";
}


/// Key mapping (S15, S16, S18–S20; S17 is checkHoldKeys). Runs while stopped, so no device is opened. Turning the monitor on is checked only when there is a device to choose for it.
/// Synthetic events do not trigger the browser's default actions (pressing a button, opening a select, moving focus), so stopping those is checked by whether the event was preventDefaulted
async function checkKeys(record: (c: string, pass: boolean, detail: unknown) => Promise<void>): Promise<void> {
  const key = (k: string, opts: KeyboardEventInit = {}, target: Element = document.activeElement ?? document.body): boolean => {
    const ev = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...opts });
    target.dispatchEvent(ev);
    return ev.defaultPrevented;
  };
  const source = () => q("#route").dataset.source;
  const monitorOn = () => pressed("monitor-on");
  const select = q<HTMLSelectElement>("#monitor");
  const keep = select.value;
  const output = q<HTMLSelectElement>("#output").value;
  const candidate = [...select.options].find((o) => o.value !== "" && o.value !== output);
  const chooseMonitor = async (value: string) => {
    select.value = value;
    select.dispatchEvent(new Event("change"));
    await sleep(300);
  };
  const toggleTwice = async (k: string, read: () => unknown, opts: KeyboardEventInit = {}, target?: () => Element) => {
    const first = read();
    const prevented1 = key(k, opts, target?.() ?? document.activeElement ?? document.body);
    await sleep(400);
    const second = read();
    const prevented2 = key(k, opts, target?.() ?? document.activeElement ?? document.body);
    await sleep(400);
    const third = read();
    return { ok: first !== second && third === first && prevented1 && prevented2, first, second, third, prevented1, prevented2 };
  };

  // S15: Space turns monitoring on and off. With no device chosen, it is refused, stays off, and a message appears
  blur();
  q("#message").textContent = "";
  await chooseMonitor("");
  key(" ");
  await sleep(400);
  const rejected = { staysOff: !monitorOn(), message: q("#message").textContent ?? "" };
  await record("S15-monitor-key-rejected", rejected.staysOff && rejected.message !== "", rejected);
  q("#message").textContent = "";
  if (candidate) {
    await chooseMonitor(candidate.value);
    blur();
    const holdBefore = [frozen("before"), frozen("after")];
    const r = await toggleTwice(" ", monitorOn);
    const holdUnchanged = frozen("before") === holdBefore[0] && frozen("after") === holdBefore[1];
    await record("S15-monitor-key", r.ok && r.first === false && holdUnchanged, { device: candidate.textContent, ...r, holdUnchanged });
  }

  // S16: Tab switches SRC and back. Shift+Tab switches it too
  {
    blur();
    const r = await toggleTwice("Tab", source);
    const textOk = q("#route").textContent === t(source() === "shifted" ? "route.shifted" : "route.original");
    const s = await toggleTwice("Tab", source, { shiftKey: true });
    await record("S16-route-key", r.ok && s.ok && textOk, { tab: r, shiftTab: s, textOk });
  }

  // S18: In the compact view, Space and Tab work, and A, S, D, and the arrows do nothing
  {
    click("view-toggle");
    await sleep(2500);
    blur();
    const compact = document.body.classList.contains("compact");
    const route = await toggleTwice("Tab", source);
    const monitor = candidate ? await toggleTwice(" ", monitorOn) : { ok: true, skipped: "no monitor device" };
    const pitch0 = shiftValue("pitch");
    const ignored = ["a", "s", "d", "ArrowUp"].map((k) => key(k));
    await sleep(200);
    const others = ignored.every((p) => !p) && shiftValue("pitch") === pitch0 && !frozen("before") && !frozen("after");
    click("view-toggle");
    await sleep(1500);
    await record("S18-compact-keys", compact && route.ok && monitor.ok && others, { compact, route, monitor, ignored, others });
  }

  // S19: With focus on a slider, a select, or a button, Space and Tab still work and stop the default action. Arrows on a slider are left to the slider
  {
    const volume = q<HTMLInputElement>("#volume");
    volume.focus();
    const onSlider = await toggleTwice("Tab", source);
    const pitch0 = shiftValue("pitch");
    const arrowPrevented = key("ArrowUp");
    const arrowLeftToSlider = !arrowPrevented && shiftValue("pitch") === pitch0;
    select.focus();
    const routeOnSelect = await toggleTwice("Tab", source);
    const monitorOnSelect = candidate ? await toggleTwice(" ", monitorOn) : { ok: true, skipped: "no monitor device" };
    const selectKept = select.value === (candidate?.value ?? "");
    const button = q<HTMLButtonElement>("#monitor-on");
    button.focus();
    const onButton = candidate ? await toggleTwice(" ", monitorOn) : { ok: true, skipped: "no monitor device" };
    blur();
    await record("S19-focus-keys", onSlider.ok && arrowLeftToSlider && routeOnSelect.ok && monitorOnSelect.ok && selectKept && onButton.ok, {
      onSlider,
      arrowLeftToSlider,
      routeOnSelect,
      monitorOnSelect,
      selectKept,
      onButton,
    });
  }

  // S20: Nothing changes while typing in a number field, with Ctrl, or on key repeat
  {
    const snapshot = () => [source(), String(monitorOn()), frozen("before"), frozen("after"), shiftValue("pitch")].join("|");
    const before = snapshot();
    const field = q<HTMLInputElement>("#pitch-num");
    field.focus();
    const inField = [" ", "Tab", "a"].map((k) => key(k, {}, field));
    blur();
    const withCtrl = [" ", "Tab", "a"].map((k) => key(k, { ctrlKey: true }));
    const repeated = [" ", "Tab", "a"].map((k) => key(k, { repeat: true }));
    await sleep(500);
    const unchanged = snapshot() === before;
    await record("S20-ignored-keys", unchanged && inField.every((p) => !p) && withCtrl.every((p) => !p), { unchanged, inField, withCtrl, repeated });
  }

  await chooseMonitor(keep);
  q("#message").textContent = "";
}


/// S17: hold keys. Holding needs analysis results, so call this while audio is running
async function checkHoldKeys(record: (c: string, pass: boolean, detail: unknown) => Promise<void>): Promise<void> {
  const key = (k: string): boolean => {
    const ev = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true });
    (document.activeElement ?? document.body).dispatchEvent(ev);
    return ev.defaultPrevented;
  };
  const source = () => q("#route").dataset.source;
  const monitorOn = () => pressed("monitor-on");
  // S17: A, S, and D hold and resume both, original, and shifted. S while held does not save. 0, 1, and 2 do nothing
  {
    blur();
    const state = () => [frozen("before"), frozen("after")].join(",");
    const steps: Record<string, string> = {};
    for (const [k, expect] of [
      ["a", "true,true"],
      ["a", "false,false"],
      ["s", "true,false"],
      ["s", "false,false"],
      ["D", "false,true"],
      ["D", "false,false"],
    ] as const) {
      key(k);
      await sleep(150);
      steps[`${k}:${expect}`] = state();
    }
    const holds = Object.entries(steps).every(([name, got]) => name.split(":")[1] === got);
    q("#message").dataset.kind = "";
    key("s");
    await sleep(1500);
    const notSaved = q("#message").dataset.kind !== "saved";
    key("s");
    await sleep(150);
    const before012 = [state(), source(), String(monitorOn())].join("|");
    const prevented012 = ["0", "1", "2"].map((k) => key(k));
    await sleep(300);
    const unchanged012 = [state(), source(), String(monitorOn())].join("|") === before012 && prevented012.every((p) => !p);
    await record("S17-hold-keys", holds && notSaved && unchanged012 && state() === "false,false", { steps, notSaved, unchanged012, prevented012 });
  }

}

/// The colour a CSS variable actually renders as (rgb(...) form)
function resolved(name: string): string {
  const probe = document.createElement("span");
  probe.style.color = `var(--${name})`;
  document.body.append(probe);
  const c = getComputedStyle(probe).color;
  probe.remove();
  return c;
}

const rgbOf = (css: string) => (css.match(/\d+/g) ?? []).slice(0, 3).map(Number);

/// Sends text and full-view keys as if the user pressed them (from the focused element)
function press(key: string, opts: KeyboardEventInit = {}, target: Element = document.activeElement ?? document.body): void {
  target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...opts }));
}

function blur(): void {
  (document.activeElement as HTMLElement | null)?.blur?.();
}

/// Emoji and symbol icons (removed in S2)
const OLD_ICONS = /[\u25B6\u25A0\u23F8\u21C4\u21BA\u21F2\u21F1\u26A0]|\u{1F3A7}|\u{1F4F7}/u;

/// S2: visible buttons have no emoji or symbol icons and are 24px high. Buttons with icons have an SVG
function checkButtons(): { lang: string; bad: unknown[]; checked: number } {
  const bad: unknown[] = [];
  let checked = 0;
  document.querySelectorAll<HTMLButtonElement>("button").forEach((b) => {
    if (b.offsetParent === null) return;
    checked++;
    const h = Math.round(b.getBoundingClientRect().height);
    const text = b.textContent ?? "";
    const needsIcon = b.dataset.icon !== undefined || ["run", "route", "view-toggle"].includes(b.id);
    const hasSvg = b.querySelector("svg.icon") !== null;
    // The pitch and formant value rows (−, +, and reset) are 32px to match the large value field
    const expected = b.closest(".value-row") ? 32 : 24;
    if (h !== expected || OLD_ICONS.test(text) || (needsIcon && !hasSvg)) bad.push({ id: b.id || b.className, h, text, needsIcon, hasSvg });
  });
  const texts = [...document.querySelectorAll<HTMLElement>("figcaption, .message, .cable")].map((e) => e.textContent ?? "");
  for (const text of texts) if (OLD_ICONS.test(text)) bad.push({ text });
  return { lang: lang(), bad, checked };
}

/// S3: the legends in the plot titles and the swatches in the F1–F4 table rows use the F1–F4 variable colours
function checkLegend(): { pass: boolean; detail: unknown } {
  const want = [1, 2, 3, 4].map((n) => resolved(`formant-${n}`));
  const legends = ["before", "after"].map((side) => {
    const items = [...q(`#${side}-panel .legend`).children] as HTMLElement[];
    return items.map((i) => ({ text: i.textContent, color: getComputedStyle(i.querySelector(".swatch")!).backgroundColor }));
  });
  const table = [...document.querySelectorAll<HTMLElement>('[data-testid="numbers"] .swatch')].map((s) => ({
    n: s.dataset.formant,
    color: getComputedStyle(s).backgroundColor,
    row: s.parentElement?.textContent,
  }));
  const legendOk = legends.every((l) => l.length === 4 && l.every((i, k) => i.text === `F${k + 1}` && i.color === want[k]));
  const tableOk = table.length === 4 && table.every((s, k) => s.n === String(k + 1) && s.color === want[k] && (s.row ?? "").startsWith(`F${k + 1}`));
  return { pass: legendOk && tableOk, detail: { want, legends, table } };
}

/// Top bar (U1): all parts are there, none is wider than the window, and the bar is 32px high. There are no key hints, and there are dividers between the hold switch groups
function checkBar(): { lang: string; pass: boolean; detail: unknown } {
  const bar = q(".bar").getBoundingClientRect();
  const ids = ["run", "freeze-all", "freeze-before", "freeze-after", "save", "settings-open", "shortcuts-open", "lang-toggle", "view-toggle"];
  const parts = ids.map((id) => {
    const el = q(`[data-testid="${id}"]`);
    const r = el.getBoundingClientRect();
    return { id, inBar: q(".bar").contains(el), visible: el.offsetParent !== null, right: Math.round(r.right) };
  });
  // No key hints on the bar (keys are learned from the ? list)
  const keyHints = q(".bar").querySelectorAll(".key").length;
  // Dividers between groups: 2 of them, about 8px right of the previous switch and about 7px left of the next name
  const switches = ["freeze-all", "freeze-before", "freeze-after"].map((id) => q(`[data-testid="${id}"]`));
  const dividers = switches.slice(1).map((sw, i) => {
    const group = sw.parentElement!;
    const line = getComputedStyle(group, "::before");
    const prevRight = switches[i].getBoundingClientRect().right;
    const name = group.querySelector<HTMLElement>(".hold-name")!.getBoundingClientRect();
    const lineWidth = parseFloat(line.width);
    const lineLeft = group.getBoundingClientRect().left;
    return {
      width: lineWidth,
      height: parseFloat(line.height),
      before: Math.round(lineLeft - prevRight),
      after: Math.round(name.left - (lineLeft + lineWidth)),
    };
  });
  const dividersOk = dividers.every((d) => d.width === 1 && d.height === 16 && Math.abs(d.before - 8) <= 1 && Math.abs(d.after - 7) <= 1);
  const messageInBar = q(".bar").contains(q("#message")) && !q(".controls").contains(q("#message"));
  const pass =
    Math.round(bar.height) === 32 &&
    parts.every((p) => p.inBar && p.visible && p.right <= window.innerWidth) &&
    keyHints === 0 &&
    dividersOk &&
    messageInBar;
  return { lang: lang(), pass, detail: { barHeight: bar.height, parts, keyHints, dividers, messageInBar, innerWidth: window.innerWidth } };
}

/// Left column (U2): only the PITCH, FORMANT, and DEVICES sections, device controls in DEVICES, and no vertical scrolling
function checkColumn(): { lang: string; pass: boolean; detail: unknown } {
  const controls = q(".controls");
  const groups = [...controls.querySelectorAll<HTMLElement>(":scope > .group")].filter((g) => g.offsetParent !== null);
  const headings = groups.map((g) => g.querySelector("h2")?.dataset.i18n ?? "");
  const devices = groups[2];
  const inDevices = ["input", "output", "monitor-on", "volume", "route"].every((id) => devices?.querySelector(`#${id}`) !== null);
  const scroll = { scrollHeight: controls.scrollHeight, clientHeight: controls.clientHeight };
  const pass = headings.join(",") === "sec.pitch,sec.formant,sec.devices" && inDevices && scroll.scrollHeight <= scroll.clientHeight;
  return { lang: lang(), pass, detail: { headings, inDevices, ...scroll, width: Math.round(controls.getBoundingClientRect().width) } };
}

/// Checks the bar and the left column in both English and Japanese (switches the language and back)
async function barAndColumnBothLanguages(): Promise<{ bar: ReturnType<typeof checkBar>[]; column: ReturnType<typeof checkColumn>[] }> {
  const bar = [checkBar()];
  const column = [checkColumn()];
  click("lang-toggle");
  await sleep(300);
  bar.push(checkBar());
  column.push(checkColumn());
  click("lang-toggle");
  await sleep(300);
  return { bar, column };
}

/// Settings panel (U3): opens with the button, has the analysis fields inside, closes with Esc and outside clicks, and returns focus to the settings button on close
async function checkSettings(record: (c: string, pass: boolean, detail: unknown) => Promise<void>): Promise<void> {
  const panel = q("#settings");
  const open = () => panel.matches(":popover-open");
  click("settings-open");
  await sleep(200);
  const opened = open();
  const expanded = q("#settings-open").getAttribute("aria-expanded");
  const fields = ["ceiling", "threshold", "relative-limit"].every((id) => panel.querySelector(`#${id}`) !== null);
  const focusInside = panel.contains(document.activeElement);
  const r = panel.getBoundingClientRect();
  const inWindow = r.left >= 0 && r.right <= window.innerWidth && r.bottom <= window.innerHeight;
  await shot("U3-settings-open");
  press("Escape");
  await sleep(200);
  const closedByEsc = !open();
  const focusBack = document.activeElement === q("#settings-open");
  click("settings-open");
  await sleep(200);
  const reopened = open();
  q("#before-gram").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  await sleep(200);
  const closedByOutside = !open();
  blur();
  await record("U3-settings", opened && expanded === "true" && fields && focusInside && inWindow && closedByEsc && focusBack && reopened && closedByOutside, {
    opened,
    expanded,
    fields,
    focusInside,
    inWindow,
    closedByEsc,
    focusBack,
    reopened,
    closedByOutside,
  });
}

export async function runSelftest(dir: string): Promise<void> {
  const results: Result[] = [];
  const record = async (criterion: string, pass: boolean, detail: unknown) => {
    results.push({ criterion, result: pass ? "PASS" : "FAIL", detail });
    await shot(criterion);
  };
  try {
    await waitFor(() => q("#cable").dataset.found !== undefined, 10_000);
    // At start, the remembered pitch and formant (in tests, the values given in the environment variable, or the defaults) are in the fields
    const expected = await invoke<[number, number] | null>("selftest_expected_shift").catch(() => null);
    const shown = [Number(q<HTMLInputElement>("#pitch-num").dataset.value), Number(q<HTMLInputElement>("#formant-num").dataset.value)];
    await record(
      "P1-shift-restored",
      expected !== null && Math.abs(shown[0] - expected[0]) < 1e-6 && Math.abs(shown[1] - expected[1]) < 1e-6,
      { expected, shown, pitchText: q<HTMLInputElement>("#pitch-num").value, formantText: q<HTMLInputElement>("#formant-num").value },
    );
    // Language switch: the text changes and switching again changes it back (the chosen language is remembered in the test config directory)
    const startEn = q("#run").textContent;
    const langBefore = document.documentElement.lang;
    click("lang-toggle");
    await sleep(200);
    const startOther = q("#run").textContent;
    const langOther = document.documentElement.lang;
    const fitOther = devicesTextsFit();
    const buttonsOther = checkButtons();
    click("lang-toggle");
    await sleep(200);
    const fitBack = devicesTextsFit();
    const buttonsBack = checkButtons();
    await record("language-toggle", langBefore !== langOther && startEn !== startOther && q("#run").textContent === startEn, {
      langBefore,
      langOther,
      startEn,
      startOther,
    });
    // The DEVICES section button labels fit on one line and within the column in both languages (D5)
    const overflowing = [...fitOther, ...fitBack].filter((f) => !f.fits);
    await record("devices-fit", overflowing.length === 0, { overflowing, checked: fitOther.length + fitBack.length });
    // No emoji or symbol icons, and every button is 24px high (S2)
    await record("S2-icons", buttonsOther.bad.length === 0 && buttonsBack.bad.length === 0 && buttonsOther.checked > 10, {
      other: buttonsOther,
      back: buttonsBack,
    });
    // The F1–F4 legend and table swatches (S3), the spectrum dB scale (S6), and the hold switch layout (U1, which replaced S9)
    const legend = checkLegend();
    await record("S3-legend", legend.pass, legend.detail);
    const dbTicksText = q<HTMLCanvasElement>("#spectrum").dataset.dbTicks;
    await record("S6-db-axis", dbTicksText === "0,-20,-40,-60,-80,-100", { dbTicks: dbTicksText });
    await checkShiftControls(record);
    {
      const { bar, column } = await barAndColumnBothLanguages();
      await record("U1-bar", bar.every((b) => b.pass), { size: "1280x860", bar });
      await record("U2-column", column.every((c) => c.pass), { size: "1280x860", column });
      await checkSettings(record);
      // U9: the status message is in one place on the bar, and the left column has no message text
      await record("U9-status", q(".bar").contains(q("#message")) && q(".controls").querySelector(".message") === null, {});
    }

    // Keyboard shortcuts: ? opens the list, other keys do nothing while it is open, and Esc closes it (S12)
    blur();
    const dialog = q<HTMLDialogElement>('[data-testid="shortcuts"]');
    const pitchBeforeDialog = q<HTMLInputElement>("#pitch-num").value;
    const frozenBeforeDialog = [frozen("before"), frozen("after")];
    press("?");
    await sleep(300);
    const opened = dialog.open;
    const focusOnClose = document.activeElement?.id === "shortcuts-close";
    await shot("S12-shortcuts");
    const routeBeforeDialog = q("#route").dataset.source;
    const monitorBeforeDialog = pressed("monitor-on");
    for (const key of [" ", "Tab", "a", "s", "d", "ArrowUp", "ArrowRight"]) press(key);
    await sleep(200);
    const unchanged =
      q<HTMLInputElement>("#pitch-num").value === pitchBeforeDialog &&
      frozen("before") === frozenBeforeDialog[0] &&
      frozen("after") === frozenBeforeDialog[1] &&
      q("#route").dataset.source === routeBeforeDialog &&
      pressed("monitor-on") === monitorBeforeDialog;
    // The rows are Space, Tab, A, S, D, the arrows, and ?, with no row for saving
    const listedKeys = [...dialog.querySelectorAll("tr")].map((tr) => [...tr.querySelectorAll("kbd")].map((k) => k.textContent).join("+"));
    const listOk =
      listedKeys.slice(0, 5).join(",") === "Space,Tab,A,S,D" && dialog.querySelector('[data-i18n="shortcuts.save"]') === null && dialog.textContent!.includes(t("shortcuts.note"));
    press("Escape");
    await sleep(200);
    const closed = !dialog.open;
    click("shortcuts-open");
    await sleep(200);
    const openedByButton = dialog.open;
    press("?");
    await sleep(200);
    await record("S12", opened && focusOnClose && unchanged && listOk && closed && openedByButton && !dialog.open, {
      opened,
      listedKeys,
      listOk,
      focusOnClose,
      unchanged,
      closed,
      openedByButton,
      closedByQuestion: !dialog.open,
    });

    // The monitor is a toggle button, not a checkbox (D3)
    await checkMonitor(record);
    // Compact view: the display on the right and the analysis and hold sections disappear, and switching again brings them back
    q("#message").textContent = "leftover";
    click("view-toggle");
    await sleep(2500);
    const compactHidden = getComputedStyle(q("#screen")).display === "none";
    const saveHidden = q<HTMLElement>("#save").offsetParent === null;
    const messageCleared = (q("#message").textContent ?? "") === "";
    // The window fits the content height of the left column, leaving no large space below
    const spare = window.innerHeight - q(".controls").getBoundingClientRect().bottom;
    // No horizontal overflow either (even with a long device name)
    const overflowX = document.documentElement.scrollWidth - window.innerWidth;
    const controlsRight = q(".controls").getBoundingClientRect().right;
    const controlsRect = q(".controls").getBoundingClientRect();
    const compactDetail = {
      innerHeight: window.innerHeight,
      outerHeight: window.outerHeight,
      controlsTop: controlsRect.top,
      controlsHeight: controlsRect.height,
      controlsScroll: q(".controls").scrollHeight,
      lastMessage: q("#message").textContent,
    };
    // U12: in the compact view, hold, save, settings, and keyboard shortcuts are hidden, the sections are in the same order as the full view, and the left column is 250px wide as in the full view
    const compactU12 = {
      hidden: ["freeze-all", "freeze-before", "freeze-after", "save", "settings-open", "shortcuts-open"].every((id) => q(`[data-testid="${id}"]`).offsetParent === null),
      column: checkColumn().detail,
      width: Math.round(q(".controls").getBoundingClientRect().width),
    };
    await shot("compact-view-compact");
    click("view-toggle");
    await sleep(1500);
    await record("compact-view", compactHidden && saveHidden && messageCleared && spare >= -2 && spare < 40 && overflowX <= 0 && controlsRight <= window.innerWidth && getComputedStyle(q("#screen")).display !== "none", {
      compactHidden,
      saveHidden,
      messageCleared,
      spare,
      overflowX,
      controlsRight,
      ...compactDetail,
    });
    {
      const c = compactU12.column as { headings: string[] };
      await record("U12-compact", compactU12.hidden && c.headings.join(",") === "sec.pitch,sec.formant,sec.devices" && compactU12.width === 250, compactU12);
    }
    // Key mapping (S15–S20)
    await checkKeys(record);

    if (q("#cable").dataset.found === "false") {
      // Without VB-CABLE, the output is empty and it cannot start (C4)
      await record("C4", disabled("run") && q<HTMLSelectElement>("#output").value === "", {
        runDisabled: disabled("run"),
      });
      // U5: a guide to choose an output appears in the plot, and the word VB-CABLE appears nowhere on screen (it is not assumed)
      const guide = q("#plot-guide").textContent ?? "";
      const mentions = /VB-?CABLE/i.test(document.body.innerText);
      await shot("U5-guide-before-start");
      await record("U5-guide", guide === t("guide.output") && !mentions, { guide, mentions });
    } else {
      const out = q<HTMLSelectElement>("#output");
      await record("cable-found", out.value !== "", { output: out.selectedOptions[0]?.textContent });
      // U5: with an output chosen and before starting, a "press start" guide appears and disappears on start
      const guideBefore = q("#plot-guide").textContent ?? "";
      await shot("U5-guide-before-start");
      click("run");
      await sleep(5000);
      const guideAfter = q("#plot-guide").textContent ?? "";
      await record("U5-guide", guideBefore === t("guide.start") && guideAfter === "", { guideBefore, guideAfter });
      // Start and stop share one button. While running it shows STOP, and IN and OUT cannot be changed (D1)
      const whileRunning = {
        running: q("#run").dataset.running,
        text: q("#run").textContent,
        inputDisabled: q<HTMLSelectElement>("#input").disabled,
        outputDisabled: q<HTMLSelectElement>("#output").disabled,
      };
      const stats = JSON.parse(q("#test-stats").textContent || "{}");
      await record("B6", stats.drawsPerSecond >= 30 && stats.receiveToDrawMs <= 100, {
        ...stats,
        note: "Delay from receiving to drawing. Does not include the delay from the sound to the analysis result arriving",
      });
      let [b, a] = f0Row();
      await record("start-0st", near(b, 150, 1) && near(a, 150, 1), { before: b, after: a });

      setNumber("pitch-num", 5);
      await sleep(3000);
      [b, a] = f0Row();
      await record("B4-shift", near(b, 150, 1) && near(a, 150 * 2 ** (5 / 12), 1.5), { before: b, after: a });

      const panelSize = () => ["before", "after"].map((s) => q(`#${s}-panel`).getBoundingClientRect()).map((r) => [r.width, r.height]);
      const sizeLive = panelSize();
      click("freeze-all");
      const held = [stateText("before"), stateText("after")];
      await sleep(1500);
      const still = [stateText("before"), stateText("after")];
      const saveEnabled = !disabled("save");
      await record("B5-freeze", frozen("before") && frozen("after") && JSON.stringify(held) === JSON.stringify(still), { held, still });

      // A held plot: a 2px purple border, a label, and HOLD on the switch. The plot size does not change (S8)
      const holdColor = resolved("hold");
      const heldLook = ["before", "after"].map((side) => {
        const panel = q(`#${side}-panel`);
        const cs = getComputedStyle(panel);
        const badge = q(`#${side}-state`);
        return {
          side,
          held: panel.dataset.held,
          border: cs.borderTopColor,
          shadow: cs.boxShadow,
          badgeText: badge.textContent,
          badgeBg: getComputedStyle(badge).backgroundColor,
        };
      });
      const switchesPressed = ["freeze-all", "freeze-before", "freeze-after"].map(pressed);
      const holdSegs = [...document.querySelectorAll<HTMLElement>(".hold-switch .seg.hold")].map((e) => getComputedStyle(e).backgroundColor);
      const sizeHeld = panelSize();
      await record(
        "S8-hold",
        heldLook.every(
          (h) => h.held === "true" && h.border === holdColor && h.shadow.includes(holdColor) && h.badgeBg === holdColor && /\d+\.\d\d s$/.test(h.badgeText ?? ""),
        ) &&
          switchesPressed.every(Boolean) &&
          holdSegs.every((c) => c === holdColor) &&
          JSON.stringify(sizeLive) === JSON.stringify(sizeHeld),
        { holdColor, heldLook, switchesPressed, holdSegs, sizeLive, sizeHeld },
      );

      // The values at the right edge equal the values in the table (S4)
      const tableFormants = (col: number) =>
        [...document.querySelectorAll<HTMLTableRowElement>('[data-testid="numbers"] tr')]
          .filter((r) => r.querySelector(".swatch"))
          .map((r) => (r.cells[col].textContent ?? "").split(" ")[0]);
      const gutter = {
        before: q<HTMLCanvasElement>("#before-gram").dataset.values,
        after: q<HTMLCanvasElement>("#after-gram").dataset.values,
        tableBefore: tableFormants(1).join(","),
        tableAfter: tableFormants(2).join(","),
      };
      await record(
        "S4-values",
        gutter.before === gutter.tableBefore && gutter.after === gutter.tableAfter && !(gutter.before ?? "").includes(",,") && (gutter.before ?? "") !== "",
        gutter,
      );

      // Harmonic dots are in the --harmonic colour and 1px high (in pixels times the scale factor) (S5)
      {
        const cv = q<HTMLCanvasElement>("#before-gram");
        const [hr, hg, hb] = rgbOf(resolved("harmonic"));
        const data = cv.getContext("2d")!.getImageData(0, 0, cv.width, cv.height).data;
        const dpr = window.devicePixelRatio || 1;
        // Look down the latest column, just before the value column
        const x = cv.width - Math.round(44 * dpr) - 1;
        let pixels = 0;
        let longest = 0;
        let run = 0;
        for (let y = 0; y < cv.height; y++) {
          const i = (y * cv.width + x) * 4;
          const hit = data[i] === hr && data[i + 1] === hg && data[i + 2] === hb;
          if (hit) {
            pixels++;
            run++;
            longest = Math.max(longest, run);
          } else run = 0;
        }
        await record("S5-harmonics", pixels > 0 && longest <= Math.max(1, Math.round(dpr)), { pixels, longest, dpr, harmonic: [hr, hg, hb] });
      }

      // Cursor over the plots: the label shows the frequency (and dB on the spectrum) at the pointer position and disappears when it leaves (S7)
      {
        const axisMax = Number(q<HTMLInputElement>("#ceiling").value) * Math.max(1, shiftValue("formant"));
        const hover = (id: string, fx: number, fy: number) => {
          const o = q<HTMLCanvasElement>(`#${id}`);
          const r = o.getBoundingClientRect();
          const x = r.width * fx;
          const y = r.height * fy;
          o.dispatchEvent(new PointerEvent("pointermove", { clientX: r.left + x, clientY: r.top + y, bubbles: true }));
          return { o, x, y, w: r.width, h: r.height };
        };
        const kHz = (s: string | undefined) => Number((s ?? "").replace(" kHz", ""));
        const dB = (s: string | undefined) => Number((s ?? "").replace("\u2212", "-").replace(" dB", ""));
        const g = hover("before-cursor", 0.5, 0.4);
        const gramText = g.o.dataset.cursorFreq;
        const gramWant = formatKHz(gramFreqAt(g.y, g.h, axisMax));
        g.o.dispatchEvent(new PointerEvent("pointerleave"));
        const sp = hover("spectrum-cursor", 0.45, 0.35);
        const l = spectrumLayout(sp.w, sp.h, 1);
        const spWant = { hz: formatKHz(spectrumFreqAt(sp.x, l, axisMax) ?? NaN), db: formatDb(spectrumDbAt(sp.y, l) ?? NaN) };
        const spText = { hz: sp.o.dataset.cursorFreq, db: sp.o.dataset.cursorDb };
        await shot("S7-cursor-hover");
        sp.o.dispatchEvent(new PointerEvent("pointerleave"));
        await sleep(50);
        const cleared = sp.o.dataset.cursorFreq === undefined && g.o.dataset.cursorFreq === undefined;
        await record(
          "S7-cursor",
          Math.abs(kHz(gramText) - kHz(gramWant)) <= 0.02 &&
            Math.abs(kHz(spText.hz) - kHz(spWant.hz)) <= 0.02 &&
            Math.abs(dB(spText.db) - dB(spWant.db)) <= 1 &&
            cleared,
          { axisMax, gramText, gramWant, spText, spWant, cleared, note: "The PNG is made only from the plot canvases, not the overlaid canvas the cursor is drawn on (composePng in png.ts)" },
        );
      }

      // Drawn dots remain when the plot size changes while held (minimising or resizing the window)
      const inked = () => {
        const cv = q<HTMLCanvasElement>("#before-gram");
        const data = cv.getContext("2d")!.getImageData(0, 0, cv.width, cv.height).data;
        let n = 0;
        for (let i = 0; i < data.length; i += 4) if (data[i] < 200) n++;
        return n;
      };
      const inkBefore = inked();
      const heightBefore = q<HTMLCanvasElement>("#before-gram").height;
      const screen = q("#screen");
      screen.style.gridTemplateRows = "1fr 3fr";
      window.dispatchEvent(new Event("resize"));
      await sleep(300);
      const heightResized = q<HTMLCanvasElement>("#before-gram").height;
      const inkResized = inked();
      screen.style.gridTemplateRows = "";
      window.dispatchEvent(new Event("resize"));
      await sleep(300);
      await record("held-survives-resize", heightResized !== heightBefore && inkBefore > 1000 && inkResized > inkBefore * 0.2, {
        heightBefore,
        heightResized,
        inkBefore,
        inkResized,
      });

      click("save");
      const saved = await waitFor(() => q("#message").dataset.kind === "saved", 10_000);
      await record("C5-save", saveEnabled && saved, { saveEnabled, message: q("#message").textContent });

      click("freeze-all");
      await sleep(300);
      await record("B5-resume", !frozen("before") && !frozen("after") && disabled("save"), {});
      const liveLook = ["before", "after"].map((side) => ({ held: q(`#${side}-panel`).dataset.held, badge: q(`#${side}-state`).textContent }));
      await record(
        "S8-resume",
        liveLook.every((l) => l.held === "false" && l.badge === "") && ["freeze-all", "freeze-before", "freeze-after"].every((id) => !pressed(id)),
        { liveLook },
      );

      click("freeze-after");
      // Holding one side does not make the title row taller with the label, and the left and right plots stay the same height (the same height means the same frequency) (S8)
      {
        const gramHeights = ["before", "after"].map((side) => q<HTMLCanvasElement>(`#${side}-gram`).getBoundingClientRect().height);
        const held = ["before", "after"].map((side) => q(`#${side}-panel`).dataset.held);
        await record("S8-one-side", gramHeights[0] === gramHeights[1] && held[0] === "false" && held[1] === "true", { gramHeights, held });
      }
      // U10, U11: holding one side overlays the held side's F1–F4 as dashed lines on the live side's plot and shows a legend in the title. It also appears in the PNG
      await checkOverlay(record);
      setNumber("pitch-num", 7);
      await sleep(3000);
      [b, a] = f0Row();
      const differ = q("#times-differ").dataset.differ === "true";
      await record("B7", near(a, 150 * 2 ** (5 / 12), 1.5) && near(b, 150, 1) && differ, { before: b, after: a, differ });

      click("freeze-all");
      const both = frozen("before") && frozen("after");
      click("freeze-all");
      const none = !frozen("before") && !frozen("after");
      await record("B8", both && none, { both, none });

      // Switching the audio sent to the output and monitor (one button for both, swapping on each press. D4)
      const routeButtons = document.querySelectorAll("button[data-testid^=\"route\"]").length;
      const first = q("#route").dataset.source;
      click("route");
      await sleep(400);
      const second = q("#route").dataset.source;
      const secondText = q("#route").textContent;
      click("route");
      await sleep(400);
      const third = q("#route").dataset.source;
      await record("routing", routeButtons === 1 && first !== second && third === first && secondText === t(second === "shifted" ? "route.shifted" : "route.original"), {
        routeButtons,
        first,
        second,
        secondText,
        third,
        message: q("#message").textContent,
      });

      // Arrow keys change the pitch and formant by fixed steps. Nothing changes while typing in a number field (S10)
      {
        blur();
        const num = (id: string) => shiftValue(id === "pitch-num" ? "pitch" : "formant");
        const p0 = num("pitch-num");
        const f0 = num("formant-num");
        press("ArrowUp");
        const p1 = num("pitch-num");
        press("ArrowUp", { shiftKey: true });
        const p2 = num("pitch-num");
        press("ArrowDown", { shiftKey: true });
        press("ArrowDown");
        const p3 = num("pitch-num");
        press("ArrowRight");
        const f1 = num("formant-num");
        press("ArrowRight", { shiftKey: true });
        const f2 = num("formant-num");
        // The slider keeps values rounded to its step (0.001), so allow a difference up to the step
        const sliderMatches = Math.abs(Number(q<HTMLInputElement>("#formant").value) - formantToSlider(shiftValue("formant"))) <= 0.001;
        press("ArrowLeft", { shiftKey: true });
        press("ArrowLeft");
        const f3 = num("formant-num");
        // With focus in the value field, only the field's ↑ works (no double change with the global keys)
        const field = q<HTMLInputElement>("#pitch-num");
        field.focus();
        press("ArrowUp", {}, field);
        const inField = num("pitch-num");
        press("ArrowDown", {}, field);
        blur();
        const close = (a: number, b: number) => Math.abs(a - b) < 1e-6;
        await record(
          "S10-arrows",
          close(p1, p0 + 0.1) && close(p2, p0 + 1.1) && close(p3, p0) && close(f1, f0 + 0.01) && close(f2, f0 + 0.06) && close(f3, f0) && sliderMatches && close(inField, p0 + 0.1),
          { p0, p1, p2, p3, f0, f1, f2, f3, sliderMatches, inField },
        );
      }

      // Hold keys (S17)
      await checkHoldKeys(record);

      // Fits on screen without vertical scrolling
      const root = document.documentElement;
      await record("fits-window", root.scrollHeight <= window.innerHeight + 1, { scrollHeight: root.scrollHeight, innerHeight: window.innerHeight });

      click("run");
      await sleep(500);
      const afterStop = {
        running: q("#run").dataset.running,
        text: q("#run").textContent,
        inputDisabled: q<HTMLSelectElement>("#input").disabled,
        outputDisabled: q<HTMLSelectElement>("#output").disabled,
      };
      const runButtons = document.querySelectorAll("#start, #stop, [data-testid=\"run\"]").length;
      await record(
        "run-toggle",
        runButtons === 1 &&
          whileRunning.running === "true" &&
          whileRunning.text === t("stop") &&
          whileRunning.inputDisabled &&
          whileRunning.outputDisabled &&
          afterStop.running === "false" &&
          afterStop.text === t("start") &&
          !afterStop.inputDisabled &&
          !afterStop.outputDisabled,
        { runButtons, whileRunning, afterStop },
      );
    }
    // Shrink to the smallest window (960×640), take a shot, and restore the size, to see in the image whether the layout fits a small window
    {
      const small = await resizeWindow(960, 640);
      await record("window-960x640", small.ok, small);
      await shot("size-960x640");
      const smallTable = await tableFitsBothLanguages();
      const smallLayout = await barAndColumnBothLanguages();
      await record("U1-bar-960x640", small.ok && smallLayout.bar.every((b) => b.pass), { bar: smallLayout.bar });
      await record("U2-column-960x640", small.ok && smallLayout.column.every((c) => c.pass), { column: smallLayout.column });
      const back = await resizeWindow(1280, 860);
      await record("window-1280x860", back.ok, back);
      const bigTable = await tableFitsBothLanguages();
      const cases = [...smallTable.map((c) => ({ size: "960x640", ...c })), ...bigTable.map((c) => ({ size: "1280x860", ...c }))];
      await record("U4-table", small.ok && back.ok && cases.every((c) => c.fits), { cases });
    }
  } catch (e) {
    await record("error", false, { message: String(e) });
  }
  const passed = results.length > 0 && results.every((r) => r.result === "PASS");
  await invoke("finish_selftest", { report: JSON.stringify(results, null, 2), passed });
  console.log(dir);
}
