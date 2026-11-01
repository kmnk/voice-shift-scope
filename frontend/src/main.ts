// 画面の入口。Tauri の command を呼び、分析の結果を channel で受けて描く。

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
/// 止めた側の重ね描き。片側だけを止めているとき、動いている側の図に止めた側の F1〜F4 を破線で重ねる
const targets = { before: new TargetOverlay($("before-target")), after: new TargetOverlay($("after-target")) };
let pending: Pair[] = [];
/// 2 つの図とスペクトルの縦（周波数）の範囲。前と後の上限の大きい方にそろえる
let axisMax = 5500;
let afterCeiling = 5500;
let devices: Devices | null = null;
let view: View = "full";
let lastStatus: Status | null = null;
/// 一度でも開始したか（始める前の案内を消すため。停止した後は最後の図を残し、案内を出さない）
let everStarted = false;
const cursors = [
  new Cursor($("before-cursor"), "gram", () => axisMax),
  new Cursor($("after-cursor"), "gram", () => axisMax),
  new Cursor($("spectrum-cursor"), "spectrum", () => axisMax),
];

/// スライダーの埋まった範囲を CSS の --from・--to に書く（style.css が溝を塗り分ける）。
/// ピッチとフォルマントは中央（既定の値）からつまみまで、音量は左端からつまみまで
function paintRange(r: HTMLInputElement): void {
  const min = Number(r.min);
  const max = Number(r.max);
  const value = Number(r.value);
  let [from, to] = fillRange(min, max, value);
  if (r.id === "volume") [from, to] = [0, max > min ? ((value - min) / (max - min)) * 100 : 0];
  r.style.setProperty("--from", `${from}%`);
  r.style.setProperty("--to", `${to}%`);
}

/// ピッチ（半音）とフォルマント（倍率）の今の値。欄の文字ではなくここが正本（欄は符号やマイナス記号を含むため）
const shift: Record<ShiftTarget, number> = { pitch: 0, formant: 1 };

/// 今の値を、値の欄（いつも同じ桁）とスライダー（フォルマントは log2 の位置）に出す
function showShift(target: ShiftTarget): void {
  const v = shift[target];
  const field = $<HTMLInputElement>(`${target}-num`);
  field.value = formatShift(target, v);
  // 試験が言葉や書式に依らずに読む値
  field.dataset.value = String(v);
  const slider = $<HTMLInputElement>(target);
  slider.value = String(target === "pitch" ? v : formantToSlider(v));
  paintRange(slider);
}

/// ピッチ・フォルマントの値を範囲で止めて丸め、欄とスライダーにそろえて出し、変換に送る
function setShift(target: ShiftTarget, value: number): void {
  shift[target] = normalize(target, value);
  showShift(target);
  void sendShift();
}

/// 小さい刻み（Shift と一緒なら大きい刻み）で上げ下げする
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

/// 帯の状態の知らせ（1 か所）。種類（data-kind）は見た目を変え、試験が言葉に依らず読むためのもの。
/// 誤り・取りこぼし（error・status）は警告のアイコンを付ける。帯に収まらない文は省略記号で切れるので、全文を説明（title）に入れる。
let messageTimer: number | undefined;
function message(text: string, kind = "error"): void {
  const m = $("message");
  if (text) setContent(m, kind === "saved" ? null : "warn", text);
  else m.replaceChildren();
  m.title = text;
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

/// 簡易表示の窓の高さ: 帯と左の列の中身の高さに、その間と body の上下の余白を足したもの
function compactContentHeight(): number {
  const controls = document.querySelector(".controls") as HTMLElement;
  const bar = document.querySelector(".bar") as HTMLElement;
  const style = getComputedStyle(document.body);
  return Math.ceil(bar.offsetHeight + parseFloat(style.rowGap) + controls.scrollHeight + parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) + 2);
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
  // VB-CABLE は出力の機器の 1 つで、いつも使う前提にしないので、有無を画面に出さない（試験が読む印だけを残す）
  $("cable").dataset.found = String(!!d.cable);
  // 始める前の案内: 一度も開始していない間だけ、何をすれば始まるかを図の中に出す
  if (d.running) everStarted = true;
  $("plot-guide").textContent = everStarted ? "" : t(d.selectedOutput ? "guide.start" : "guide.output");
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
  // 右端の値は、表と同じ 250 ms の中央値（止めている側は止めた時点の値）。
  // 片側だけを止めているときは、動いている側の図に止めた側の値を目標として重ねる（両側を止めた・両側が動いているときは重ねない）
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

/// 言葉を替えたときに、動的に書いた文も書き直す。
function refreshTexts(): void {
  applyStatic();
  // 表示の切り替えはアイコンだけのボタン。名前は説明と読み上げに持たせる
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
  // 設定の板は全体の表示だけ（簡易表示では分析が止まるため）
  closeSettings();
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

/// ピッチ・フォルマントの欄の操作: スライダー、値の欄への入力、欄の中の ↑↓、ホイール、− と + のボタン
function wireShift(target: ShiftTarget): void {
  const slider = $<HTMLInputElement>(target);
  const field = $<HTMLInputElement>(`${target}-num`);
  slider.addEventListener("input", () => {
    const position = Number(slider.value);
    setShift(target, target === "pitch" ? position : sliderToFormant(position));
  });
  // 欄を押すと文字を選ぶ。Enter かフォーカスが外れると値にし、読めない文字なら前の値に戻す
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
  // ホイール: 上で増え下で減る。Shift と一緒のとき Chromium は横のスクロール（deltaX）にすることがあるので両方を見る。
  // タッチパッドで細かい事象が続くと速く動きすぎるので、100 ms に 1 刻みに間引く
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
  wireShift("pitch");
  wireShift("formant");
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
  // 値の欄の左右の − と +（Shift と一緒に押すと大きい刻み）と、元に戻すボタン
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
    // 前の表示の知らせ（保存しましたなど）は持ち越さない
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
  // 動いている間に機器が無くなったときなど、音声の側の誤りを出す
  setInterval(async () => {
    const s = await invoke<Status>("status").catch(() => null);
    if (s) showStatus(s);
  }, 1000);
}

/// 設定の板（分析の設定）。帯の設定のボタンで開く。Esc・板の外を押す・ボタンでもう一度押すと閉じる。
/// 閉じる振る舞いはブラウザーの light dismiss に任せず、ここで持つ（試験が送るキーとクリックでも同じに動くように）
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
  // 決まったアイコンを置く（言葉を替えても変わらない）
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
  // 簡易表示で起動したときも、中身の高さに窓を合わせる
  if (prefs.view === "compact") void call("set_view", { view: "compact", contentHeight: compactContentHeight() });
  const d = await call<Devices>("list_devices");
  if (d) {
    showDevices(d);
    // 覚えておいたピッチとフォルマントを欄とスライダーに出す（声を変える処理には起動したときに渡してある）
    shift.pitch = normalize("pitch", d.shift.semitones);
    shift.formant = normalize("formant", d.shift.formant);
    showShift("pitch");
    showShift("formant");
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
