// 画面の入口。Tauri の command を呼び、分析の結果を channel で受けて描く。

import { Channel, invoke } from "@tauri-apps/api/core";
import { composePng, fileName } from "./png";
import { runSelftest } from "./selftest";
import { Spectrogram, drawSpectrum, renderTable } from "./render";
import { FreezeStore } from "./store";
import type { Devices, Pair, Side } from "./types";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const store = new FreezeStore();
const grams = { before: new Spectrogram($("before-gram")), after: new Spectrogram($("after-gram")) };
let pending: Pair[] = [];
/// 2 つの図とスペクトルの縦（周波数）の範囲。前と後の上限の大きい方にそろえる
let axisMax = 5500;
let devices: Devices | null = null;

function message(text: string): void {
  $("message").textContent = text;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    const err = e as { message?: string };
    message(err.message ?? String(e));
    return undefined;
  }
}

function fillSelect(select: HTMLSelectElement, list: { id: string; name: string }[], selected: string | null): void {
  select.innerHTML = `<option value="">（選んでいない）</option>` + list.map((d) => `<option value="${d.id}">${d.name}</option>`).join("");
  select.value = selected ?? "";
}

function showDevices(d: Devices): void {
  devices = d;
  fillSelect($("input"), d.inputs, d.selectedInput);
  fillSelect($("monitor"), d.monitors, d.selectedMonitor);
  ($("monitor-on") as HTMLInputElement).checked = d.monitorEnabled;
  ($("volume") as HTMLInputElement).value = String(d.monitorVolume);
  const cable = $("cable");
  if (d.cable) {
    cable.textContent = `出力の先: ${d.cable.playback}（他のソフトでは ${d.cable.recording} を選ぶ）`;
    cable.classList.remove("missing");
  } else {
    cable.innerHTML =
      '出力の先の VB-CABLE が見つかりません。VB-CABLE を入れてから起動し直してください（入手先: https://vb-audio.com/Cable/ ）。機器の名前を変えていたら戻してください';
    cable.classList.add("missing");
  }
  ($("start") as HTMLButtonElement).disabled = d.running || !d.cable;
  ($("stop") as HTMLButtonElement).disabled = !d.running;
  ($("input") as HTMLSelectElement).disabled = d.running;
}

function updateFreezeButtons(): void {
  for (const side of ["before", "after"] as const) {
    const frozen = store.isFrozen(side);
    $(`freeze-${side}`).setAttribute("aria-pressed", String(frozen));
    $(`freeze-${side}`).textContent = `${side === "before" ? "前" : "後"}を${frozen ? "戻す" : "止める"}`;
    const v = store.view(side);
    const state = $(`${side}-state`);
    state.textContent = frozen && v ? `止めている（${v.time.toFixed(2)} 秒）` : "";
    state.classList.toggle("frozen", frozen);
  }
  const all = store.isFrozen("before") && store.isFrozen("after");
  $("freeze-all").textContent = all ? "全体を戻す" : "全体を止める";
  ($("save") as HTMLButtonElement).disabled = !store.anyFrozen();
}

function toggle(side: Side | "all"): void {
  if (side === "all") store.toggleAll();
  else store.toggle(side);
  updateFreezeButtons();
  draw();
}

function draw(): void {
  const before = store.view("before");
  const after = store.view("after");
  drawSpectrum($("spectrum"), before, after, axisMax);
  renderTable($("numbers"), before, after, store.timesDiffer());
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
      $("after-ceiling").textContent = pair.settings.afterCeiling.toFixed(0);
      for (const side of ["before", "after"] as const) {
        grams[side].prepare(axisMax);
        if (!store.isFrozen(side)) grams[side].push(pair[side]);
      }
    }
    const arrived = (batch[batch.length - 1] as Pair & { arrivedAt: number }).arrivedAt;
    stats.lastLagMs = now - arrived;
    draw();
    stats.frames++;
  }
  if (now - stats.since >= 1000) {
    const out = $("test-stats");
    out.textContent = JSON.stringify({
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
  if (path) message(`保存しました: ${path}`);
}

function wire(): void {
  $("start").addEventListener("click", () => void start());
  $("stop").addEventListener("click", async () => {
    const d = await call<Devices>("stop");
    if (d) showDevices(d);
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
  window.addEventListener("resize", () => {
    grams.before.prepare(axisMax);
    grams.after.prepare(axisMax);
    draw();
  });
  $("freeze-all").addEventListener("click", () => toggle("all"));
  $("freeze-before").addEventListener("click", () => toggle("before"));
  $("freeze-after").addEventListener("click", () => toggle("after"));
  $("save").addEventListener("click", () => void save());
  document.addEventListener("keydown", (e) => {
    const target = e.target as HTMLElement;
    if (target.tagName === "INPUT" || target.tagName === "SELECT") return;
    if (e.key === " ") {
      e.preventDefault();
      toggle("all");
    } else if (e.key === "1") toggle("before");
    else if (e.key === "2") toggle("after");
  });
  // 動いている間に機器が無くなったときなど、音声の側の誤りを出す
  setInterval(async () => {
    const s = await invoke<{
      running: boolean;
      errors: string[];
      inputXruns: number;
      outputXruns: number;
      monitorXruns: number;
      lastInputXrunSeconds: number;
    }>("status").catch(() => null);
    if (!s) return;
    const parts = [...new Set(s.errors)];
    const xruns = [
      s.inputXruns > 0 ? `入力 ${s.inputXruns} 回（最後は開始から ${s.lastInputXrunSeconds.toFixed(1)} 秒）` : "",
      s.outputXruns > 0 ? `出力 ${s.outputXruns} 回` : "",
      s.monitorXruns > 0 ? `モニター ${s.monitorXruns} 回` : "",
    ].filter(Boolean);
    if (xruns.length > 0) parts.push(`音の取りこぼし: ${xruns.join("、")}。続くときは別の機器を選ぶか、他のソフトを閉じてください`);
    if (parts.length > 0) message(parts.join(" / "));
  }, 1000);
}

wire();
updateFreezeButtons();
grams.before.prepare(axisMax);
grams.after.prepare(axisMax);
draw();
void call<Devices>("list_devices").then((d) => d && showDevices(d));
requestAnimationFrame(loop);
void invoke<string | null>("selftest_config").then((dir) => {
  if (dir) void runSelftest(dir);
});
