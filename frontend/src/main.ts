// 画面の入口。Tauri の command を呼び、分析の結果を channel で受けて描く。

import { Channel, invoke } from "@tauri-apps/api/core";
import { applyStatic, hasKey, lang, setLang, t, type Lang } from "./i18n";
import { composePng, fileName } from "./png";
import { Spectrogram, drawSpectrum, renderTable } from "./render";
import { runSelftest } from "./selftest";
import { FreezeStore } from "./store";
import type { Devices, Pair, Side } from "./types";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

type View = "full" | "compact";

const store = new FreezeStore();
const grams = { before: new Spectrogram($("before-gram")), after: new Spectrogram($("after-gram")) };
let pending: Pair[] = [];
/// 2 つの図とスペクトルの縦（周波数）の範囲。前と後の上限の大きい方にそろえる
let axisMax = 5500;
let afterCeiling = 5500;
let devices: Devices | null = null;
let view: View = "full";
let lastStatus: Status | null = null;

interface Status {
  running: boolean;
  errors: string[];
  inputXruns: number;
  outputXruns: number;
  monitorXruns: number;
  lastInputXrunSeconds: number;
}

/// 画面の下の知らせ。種類（data-kind）は試験が言葉に依らず読むためのもの。
let messageTimer: number | undefined;
function message(text: string, kind = "error"): void {
  const m = $("message");
  m.textContent = text;
  m.dataset.kind = text ? kind : "";
  // 「保存しました」は数秒で消す（誤りと取りこぼしは、続いている間は出し続ける）
  window.clearTimeout(messageTimer);
  if (kind === "saved") messageTimer = window.setTimeout(() => message(""), 5000);
}

/// 簡易表示で窓の高さを決めたあと、実際の窓の中の高さと中身がずれていれば（窓の縁の分など）、その差だけ直す。
/// 何度も直し続けないよう、1 回の切り替えにつき 2 回まで。
let compactFixes = 0;
function fitCompactWindow(): void {
  if (view !== "compact" || compactFixes >= 2) return;
  const diff = compactContentHeight() - window.innerHeight;
  if (Math.abs(diff) <= 2) return;
  compactFixes++;
  void call("set_view", { view: "compact", contentHeight: compactContentHeight() + diff });
}

/// 簡易表示の窓の高さ: 左の列の中身の高さに、body の上下の余白を足したもの
function compactContentHeight(): number {
  const controls = document.querySelector(".controls") as HTMLElement;
  const style = getComputedStyle(document.body);
  return Math.ceil(controls.scrollHeight + parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) + 2);
}

/// command の誤りを、今の言葉で出す（domain の誤りは名前で引き、無ければ届いた文のまま）。
function errorText(e: unknown): string {
  const err = e as { code?: string; message?: string };
  const key = `err.${err.code ?? ""}`;
  if (err.code && hasKey(key)) return t(key);
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
  ($("monitor-on") as HTMLInputElement).checked = d.monitorEnabled;
  ($("volume") as HTMLInputElement).value = String(d.monitorVolume);
  fillSelect($("output"), d.outputs, d.selectedOutput);
  // VB-CABLE が見つからないときだけ知らせる（見つかったときの説明は出さない）
  const cable = $("cable");
  cable.textContent = d.cable ? "" : t("cable.missing");
  cable.classList.toggle("missing", !d.cable);
  cable.dataset.found = String(!!d.cable);
  // 出力とモニターには同じ音を流す（モニターは出しているものを確かめるため。2026-10-18 に利用者が 1 つにした）
  document.querySelectorAll<HTMLButtonElement>("button[data-route]").forEach((b) => {
    b.setAttribute("aria-pressed", String(b.dataset.source === d.outputSource));
  });
  ($("start") as HTMLButtonElement).disabled = d.running || !d.selectedOutput;
  ($("output") as HTMLSelectElement).disabled = d.running;
  ($("stop") as HTMLButtonElement).disabled = !d.running;
  ($("input") as HTMLSelectElement).disabled = d.running;
}

function updateFreezeButtons(): void {
  for (const side of ["before", "after"] as const) {
    const frozen = store.isFrozen(side);
    const button = $(`freeze-${side}`);
    button.setAttribute("aria-pressed", String(frozen));
    const name = t(side === "before" ? "original" : "shifted");
    button.textContent = frozen ? t("resume.side", { side: name }) : t("hold.side", { side: name });
    const v = store.view(side);
    const state = $(`${side}-state`);
    state.textContent = frozen && v ? t("state.held", { t: v.time.toFixed(2) }) : "";
    state.classList.toggle("frozen", frozen);
    state.dataset.frozen = String(frozen);
  }
  const all = store.isFrozen("before") && store.isFrozen("after");
  $("freeze-all").textContent = all ? t("resume.all") : t("hold.all");
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
  const differ = $("times-differ");
  differ.textContent = store.timesDiffer() ? t("times.differ") : "";
  differ.dataset.differ = String(store.timesDiffer());
  $("ceiling-note").textContent = t("ceiling.note", { hz: afterCeiling.toFixed(0) });
}

/// 言葉を替えたときに、動的に書いた文も書き直す。
function refreshTexts(): void {
  applyStatic();
  $("view-toggle").textContent = view === "full" ? "⇲ " + (lang() === "ja" ? "簡易" : "COMPACT") : "⇱ " + (lang() === "ja" ? "全体" : "FULL");
  $("view-toggle").title = view === "full" ? (lang() === "ja" ? "声を変えるだけの簡易表示にする" : "Voice changer only") : lang() === "ja" ? "グラフと数値も出す" : "Show plots and values";
  if (devices) showDevices(devices);
  updateFreezeButtons();
  if (lastStatus) showStatus(lastStatus);
  draw();
}

function applyView(v: View): void {
  view = v;
  document.body.classList.toggle("compact", v === "compact");
  refreshTexts();
  if (v === "full") {
    grams.before.prepare(axisMax);
    grams.after.prepare(axisMax);
  }
}

// 試験のための数（表示の更新の回数と、受けてから描くまでの遅れ）
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
    semitones: Number(($("pitch-num") as HTMLInputElement).value),
    formant: Number(($("formant-num") as HTMLInputElement).value),
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

function link(range: string, num: string): void {
  const r = $(range) as HTMLInputElement;
  const n = $(num) as HTMLInputElement;
  r.addEventListener("input", () => {
    n.value = r.value;
    void sendShift();
  });
  n.addEventListener("change", () => {
    r.value = n.value;
    void sendShift();
  });
}

async function save(): Promise<void> {
  if (!store.anyFrozen()) return;
  const now = new Date();
  const bytes = await composePng(
    [$("before-gram") as HTMLCanvasElement, $("after-gram") as HTMLCanvasElement],
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
  $("start").addEventListener("click", () => void start());
  $("stop").addEventListener("click", async () => {
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
  const monitorChange = async () => {
    const id = ($("monitor") as HTMLSelectElement).value || null;
    const enabled = ($("monitor-on") as HTMLInputElement).checked;
    const d = await call<Devices>("set_monitor", { enabled, id });
    if (d) showDevices(d);
    else if (devices) showDevices(devices);
  };
  $("monitor").addEventListener("change", () => void monitorChange());
  $("monitor-on").addEventListener("change", () => void monitorChange());
  $("volume").addEventListener("input", (e) => void call("set_monitor_volume", { volume: Number((e.target as HTMLInputElement).value) }));
  link("pitch", "pitch-num");
  link("formant", "formant-num");
  document.querySelectorAll<HTMLButtonElement>("button[data-route]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (!devices) return;
      const source = b.dataset.source as Devices["outputSource"];
      devices.outputSource = source;
      devices.monitorSource = source;
      showDevices(devices);
      const d = await call<Devices>("set_routing", { output: source, monitor: source });
      if (d) showDevices(d);
    }),
  );
  // 細かく合わせるための、決まった幅で上げ下げするボタンと、元に戻すボタン
  const defaults: Record<string, number> = { pitch: 0, formant: 1 };
  const decimals: Record<string, number> = { pitch: 1, formant: 2 };
  const setValue = (target: string, value: number) => {
    const r = $(target) as HTMLInputElement;
    const n = $(`${target}-num`) as HTMLInputElement;
    const clamped = Math.min(Number(r.max), Math.max(Number(r.min), value));
    n.value = clamped.toFixed(decimals[target]);
    r.value = n.value;
    void sendShift();
  };
  document.querySelectorAll<HTMLButtonElement>("button[data-step]").forEach((b) =>
    b.addEventListener("click", () => {
      const target = b.dataset.target!;
      setValue(target, Number(($(`${target}-num`) as HTMLInputElement).value) + Number(b.dataset.step));
    }),
  );
  document.querySelectorAll<HTMLButtonElement>("button.reset").forEach((b) =>
    b.addEventListener("click", () => setValue(b.dataset.target!, defaults[b.dataset.target!])),
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
    // 前の表示の知らせ（保存しましたなど）は持ち越さない
    message("");
    compactFixes = 0;
    applyView(next);
    void call("set_view", { view: next, contentHeight: next === "compact" ? compactContentHeight() : null });
  });
  window.addEventListener("resize", () => {
    if (view === "compact") {
      fitCompactWindow();
      return;
    }
    grams.before.prepare(axisMax);
    grams.after.prepare(axisMax);
    draw();
  });
  document.addEventListener("keydown", (e) => {
    const target = e.target as HTMLElement;
    if (view !== "full" || target.tagName === "INPUT" || target.tagName === "SELECT") return;
    if (e.key === " ") {
      e.preventDefault();
      toggle("all");
    } else if (e.key === "1") toggle("before");
    else if (e.key === "2") toggle("after");
  });
  // 動いている間に機器が無くなったときなど、音声の側の誤りを出す
  setInterval(async () => {
    const s = await invoke<Status>("status").catch(() => null);
    if (s) showStatus(s);
  }, 1000);
}

async function boot(): Promise<void> {
  wire();
  const prefs = await invoke<{ language: Lang; view: View }>("get_ui_prefs").catch(() => ({ language: "en" as Lang, view: "full" as View }));
  setLang(prefs.language);
  applyView(prefs.view);
  // 簡易表示で起動したときも、中身の高さに窓を合わせる
  if (prefs.view === "compact") void call("set_view", { view: "compact", contentHeight: compactContentHeight() });
  const d = await call<Devices>("list_devices");
  if (d) {
    showDevices(d);
    // 覚えておいた分析の設定を欄に入れる
    ($("ceiling") as HTMLInputElement).value = String(d.analysis.ceiling);
    ($("threshold") as HTMLInputElement).value = String(d.analysis.threshold);
    ($("relative-limit") as HTMLInputElement).value = String(d.analysis.relativeLimit);
  }
  requestAnimationFrame(loop);
  const dir = await invoke<string | null>("selftest_config").catch(() => null);
  if (dir) void runSelftest(dir);
}

void boot();
