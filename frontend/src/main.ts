// 画面の入口。Tauri の command を呼び、分析の結果を channel で受けて描く。

import { Channel, invoke } from "@tauri-apps/api/core";
import { applyStatic, hasKey, lang, setLang, t, type Lang } from "./i18n";
import { Cursor } from "./cursor";
import { icon, setContent, type IconName } from "./icons";
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
const cursors = [
  new Cursor($("before-cursor"), "gram", () => axisMax),
  new Cursor($("after-cursor"), "gram", () => axisMax),
  new Cursor($("spectrum-cursor"), "spectrum", () => axisMax),
];

/// スライダーの埋まった部分の割合を CSS の --fill に書く（style.css が溝を塗り分ける）
function paintRange(r: HTMLInputElement): void {
  const min = Number(r.min);
  const max = Number(r.max);
  const pct = max > min ? ((Number(r.value) - min) / (max - min)) * 100 : 0;
  r.style.setProperty("--fill", `${Math.min(100, Math.max(0, pct))}%`);
}

/// ピッチ・フォルマントの値を、数の欄とスライダーにそろえて入れ、変換に送る（範囲で止める）
const decimals: Record<string, number> = { pitch: 1, formant: 2 };
function setShift(target: "pitch" | "formant", value: number): void {
  const r = $<HTMLInputElement>(target);
  const n = $<HTMLInputElement>(`${target}-num`);
  const clamped = Math.min(Number(r.max), Math.max(Number(r.min), value));
  n.value = clamped.toFixed(decimals[target]);
  r.value = n.value;
  paintRange(r);
  void sendShift();
}

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

/// 簡易表示で、左の列の中身の高さが変わったら（窓が狭くなって文が折り返すなど）、窓の高さを合わせ直す。
/// 窓の縁の分は Rust の側が直す。何度も直し続けないよう、1 回の切り替えにつき 4 回まで。
let compactFixes = 0;
function fitCompactWindow(): void {
  if (view !== "compact" || compactFixes >= 4) return;
  const wanted = compactContentHeight();
  if (Math.abs(wanted - window.innerHeight) <= 2) return;
  compactFixes++;
  void call("set_view", { view: "compact", contentHeight: wanted });
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
  $("monitor-on").setAttribute("aria-pressed", String(d.monitorEnabled));
  ($("volume") as HTMLInputElement).value = String(d.monitorVolume);
  paintRange($("volume"));
  fillSelect($("output"), d.outputs, d.selectedOutput);
  // VB-CABLE が見つからないときだけ知らせる（見つかったときの説明は出さない）
  const cable = $("cable");
  cable.textContent = d.cable ? "" : t("cable.missing");
  cable.classList.toggle("missing", !d.cable);
  cable.dataset.found = String(!!d.cable);
  // 出力とモニターには同じ音を流す（モニターは出しているものを確かめるため。2026-10-18 に利用者が 1 つにした）。
  // ボタンは 1 つで、今の値を出す
  const route = $("route");
  route.dataset.source = d.outputSource;
  setContent(route, "route", t(d.outputSource === "shifted" ? "route.shifted" : "route.original"));
  // 開始と停止は 1 つのボタン。出力を選んでいない間は開始できない
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
    // 止めた図は枠と札で分かるようにする（style.css の .panel[data-held]）
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
  // 右端の値は、表と同じ 250 ms の中央値（止めている側は止めた時点の値）
  for (const [side, v] of [["before", before], ["after", after]] as const) {
    const values = [0, 1, 2, 3].map((i) => v?.summary?.formants[i]?.frequency ?? null);
    grams[side].setValues(values, v?.summary?.stale ?? false);
  }
  const differ = $("times-differ");
  if (store.timesDiffer()) setContent(differ, "warn", t("times.differ"));
  else differ.replaceChildren();
  differ.dataset.differ = String(store.timesDiffer());
  $("ceiling-note").textContent = t("ceiling.note", { hz: afterCeiling.toFixed(0) });
}

/// 言葉を替えたときに、動的に書いた文も書き直す。
function refreshTexts(): void {
  applyStatic();
  setContent($("view-toggle"), view === "full" ? "compact" : "full", t(view === "full" ? "view.compact" : "view.full"));
  $("view-toggle").title = t(view === "full" ? "view.compact.title" : "view.full.title");
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
    paintRange(r);
    void sendShift();
  });
  n.addEventListener("change", () => {
    r.value = n.value;
    paintRange(r);
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
  // モニターの ON/OFF はトグルボタン。押したら反対の値を送り、拒まれたら今の値の表示に戻す
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
  link("pitch", "pitch-num");
  link("formant", "formant-num");
  // 流す音のボタンは、押すと反対側に切り替える
  $("route").addEventListener("click", async () => {
    if (!devices) return;
    const source: Devices["outputSource"] = devices.outputSource === "shifted" ? "original" : "shifted";
    devices.outputSource = source;
    devices.monitorSource = source;
    showDevices(devices);
    const d = await call<Devices>("set_routing", { output: source, monitor: source });
    if (d) showDevices(d);
  });
  // 細かく合わせるための、決まった幅で上げ下げするボタンと、元に戻すボタン
  const defaults: Record<string, number> = { pitch: 0, formant: 1 };
  type Target = "pitch" | "formant";
  document.querySelectorAll<HTMLButtonElement>("button[data-step]").forEach((b) =>
    b.addEventListener("click", () => {
      const target = b.dataset.target as Target;
      setShift(target, Number(($(`${target}-num`) as HTMLInputElement).value) + Number(b.dataset.step));
    }),
  );
  document.querySelectorAll<HTMLButtonElement>("button.reset").forEach((b) =>
    b.addEventListener("click", () => setShift(b.dataset.target as Target, defaults[b.dataset.target!])),
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
  new ResizeObserver(() => fitCompactWindow()).observe(document.querySelector(".controls") as HTMLElement);
  window.addEventListener("resize", () => {
    if (view === "compact") {
      fitCompactWindow();
      return;
    }
    grams.before.prepare(axisMax);
    grams.after.prepare(axisMax);
    draw();
    for (const c of cursors) c.draw();
  });
  wireShortcuts();
  // 動いている間に機器が無くなったときなど、音声の側の誤りを出す
  setInterval(async () => {
    const s = await invoke<Status>("status").catch(() => null);
    if (s) showStatus(s);
  }, 1000);
}

/// キー操作の一覧を開く・閉じる。閉じたら、開く前にフォーカスのあった所へ戻す
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

/// キー。全体の表示のときだけ効き、数の欄・選ぶ欄・スライダーに入力しているときと、Ctrl・Alt・Meta と一緒のときは効かない。
/// 一覧を開いている間は、一覧を閉じるキー（? と Esc）だけが効く。
function wireShortcuts(): void {
  const dialog = $<HTMLDialogElement>("shortcuts");
  $("shortcuts-open").addEventListener("click", openShortcuts);
  $("shortcuts-close").addEventListener("click", closeShortcuts);
  // 後ろの暗い所（dialog の要素そのもの）を押すと閉じる
  dialog.addEventListener("click", (e) => {
    if (e.target === dialog) closeShortcuts();
  });
  dialog.addEventListener("close", () => {
    focusBeforeShortcuts?.focus();
    focusBeforeShortcuts = null;
  });
  document.addEventListener("keydown", (e) => {
    if (e.ctrlKey || e.altKey || e.metaKey) return;
    if (dialog.open) {
      if (e.key === "?" || e.key === "Escape") {
        e.preventDefault();
        closeShortcuts();
      }
      return;
    }
    const target = e.target as HTMLElement;
    if (view !== "full" || ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName)) return;
    const shiftValue = (name: "pitch" | "formant", fine: number, coarse: number, sign: number) => {
      e.preventDefault();
      const current = Number($<HTMLInputElement>(`${name}-num`).value);
      setShift(name, current + sign * (e.shiftKey ? coarse : fine));
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
  // 決まったアイコンを置く（言葉を替えても変わらない）
  document.querySelectorAll<HTMLElement>("[data-icon]").forEach((el) => {
    if (!el.querySelector(":scope > svg.icon")) el.prepend(icon(el.dataset.icon as IconName));
  });
  document.querySelectorAll<HTMLInputElement>('input[type="range"]').forEach(paintRange);
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
    document.querySelectorAll<HTMLInputElement>('input[type="range"]').forEach(paintRange);
  }
  requestAnimationFrame(loop);
  const dir = await invoke<string | null>("selftest_config").catch(() => null);
  if (dir) void runSelftest(dir);
}

void boot();
