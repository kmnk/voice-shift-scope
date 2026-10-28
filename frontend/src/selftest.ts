// 画面の通しの試験（`--ui-selftest <dir>` で起動したときだけ動く）。
// 利用者と同じ操作（ボタンを押す・値を入れる）を DOM で順に行い、画面に出たものを読んで判定する。
// 言葉に依らないように、文ではなく data-* の印で状態を読む。結果は <dir>/ui-report.json に書き、アプリは終わる。

import { invoke } from "@tauri-apps/api/core";
import { lang, t } from "./i18n";

type Result = { criterion: string; result: "PASS" | "FAIL"; detail: unknown };

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const q = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;
const click = (testid: string) => q<HTMLButtonElement>(`[data-testid="${testid}"]`).click();
const disabled = (testid: string) => q<HTMLButtonElement>(`[data-testid="${testid}"]`).disabled;
const frozen = (side: string) => q(`#${side}-state`).dataset.frozen === "true";
const stateText = (side: string) => q(`#${side}-state`).textContent ?? "";
const near = (v: number, target: number, pct: number) => Math.abs(v / target - 1) * 100 <= pct;
const pressed = (testid: string) => q(`[data-testid="${testid}"]`).getAttribute("aria-pressed") === "true";

// 受け入れ基準の項目を判定した時点の窓の画像を、Rust 側が <dir>/shots/<name>.png に書く（Windows だけ）。
// 撮れないことは試験の判定に入れない。
async function shot(name: string): Promise<void> {
  try {
    await invoke<string>("selftest_shot", { name });
  } catch {
    // 撮れないことは試験の失敗にしない
  }
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

/// DEVICES 欄のボタンが、取りうる文言のどれでも 1 行に収まり、欄からはみ出さないか（今の言葉で測る）。
/// 測るために文言を一時的に入れ替え、終わったら戻す。
function devicesTextsFit(): { lang: string; id: string; text: string; fits: boolean; scrollWidth: number; clientWidth: number; height: number }[] {
  const group = q("#run").closest(".group") as HTMLElement;
  const groupRight = group.getBoundingClientRect().right;
  const cases: [string, string[]][] = [
    ["run", [t("start"), t("stop")]],
    ["monitor-on", [t("monitor.on")]],
    ["route", [t("route.shifted"), t("route.original")]],
  ];
  const out = [];
  for (const [id, texts] of cases) {
    const b = q<HTMLButtonElement>(`#${id}`);
    const keep = b.textContent;
    for (const text of texts) {
      b.textContent = text;
      const rect = b.getBoundingClientRect();
      const fits = b.scrollWidth <= b.clientWidth + 1 && rect.right <= groupRight && rect.height <= 25;
      out.push({ lang: lang(), id, text, fits, scrollWidth: b.scrollWidth, clientWidth: b.clientWidth, height: rect.height });
    }
    b.textContent = keep;
  }
  return out;
}

/// モニターの ON/OFF のボタン。機器を選ばずに ON にすると拒まれ、表示が OFF に戻る。
/// モニターに選べる機器（出力と別のもの）があれば、ON と OFF が切り替わることも見る。止まっている間に行うので機器は開かない。
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
  // 元の選択に戻す（OFF のまま）
  select.value = keep;
  select.dispatchEvent(new Event("change"));
  await sleep(300);
  q("#message").textContent = "";
}

export async function runSelftest(dir: string): Promise<void> {
  const results: Result[] = [];
  const record = async (criterion: string, pass: boolean, detail: unknown) => {
    results.push({ criterion, result: pass ? "PASS" : "FAIL", detail });
    await shot(criterion);
  };
  try {
    await waitFor(() => q("#cable").dataset.found !== undefined, 10_000);
    // 言葉の切り替え: 文が替わり、もう一度で戻る（選んだ言葉は試験の設定のディレクトリに覚える）
    const startEn = q("#run").textContent;
    const langBefore = document.documentElement.lang;
    click("lang-toggle");
    await sleep(200);
    const startOther = q("#run").textContent;
    const langOther = document.documentElement.lang;
    const fitOther = devicesTextsFit();
    click("lang-toggle");
    await sleep(200);
    const fitBack = devicesTextsFit();
    await record("language-toggle", langBefore !== langOther && startEn !== startOther && q("#run").textContent === startEn, {
      langBefore,
      langOther,
      startEn,
      startOther,
    });
    // DEVICES 欄のボタンの文言が、どちらの言葉でも 1 行に収まり、列からはみ出さない（spec の D5）
    const overflowing = [...fitOther, ...fitBack].filter((f) => !f.fits);
    await record("devices-fit", overflowing.length === 0, { overflowing, checked: fitOther.length + fitBack.length });

    // モニターはチェックボックスでなくトグルボタン（spec の D3）
    await checkMonitor(record);
    // 簡易表示: 右の表示と分析・一時停止の欄が消え、もう一度で戻る
    q("#message").textContent = "leftover";
    click("view-toggle");
    await sleep(2500);
    const compactHidden = getComputedStyle(q("#screen")).display === "none";
    const saveHidden = q<HTMLElement>("#save").offsetParent === null;
    const messageCleared = (q("#message").textContent ?? "") === "";
    // 窓が左の列の中身の高さに合い、下に大きな余白が残らない
    const spare = window.innerHeight - q(".controls").getBoundingClientRect().bottom;
    // 横にもはみ出さない（長い機器の名前でも）
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

    if (q("#cable").dataset.found === "false") {
      // VB-CABLE が無いときは、出力が空で開始できず、知らせが出る
      await record("C4", disabled("run") && q<HTMLSelectElement>("#output").value === "" && (q("#cable").textContent ?? "") !== "", {
        cable: q("#cable").textContent,
        runDisabled: disabled("run"),
      });
    } else {
      const out = q<HTMLSelectElement>("#output");
      await record("cable-found", out.value !== "" && (q("#cable").textContent ?? "") === "", { output: out.selectedOptions[0]?.textContent, note: q("#cable").textContent });
      click("run");
      await sleep(5000);
      // 開始と停止は 1 つのボタン。動いている間は STOP を出し、IN と OUT を選べない（spec の D1）
      const whileRunning = {
        running: q("#run").dataset.running,
        text: q("#run").textContent,
        inputDisabled: q<HTMLSelectElement>("#input").disabled,
        outputDisabled: q<HTMLSelectElement>("#output").disabled,
      };
      const stats = JSON.parse(q("#test-stats").textContent || "{}");
      await record("B6", stats.drawsPerSecond >= 30 && stats.receiveToDrawMs <= 100, {
        ...stats,
        note: "受けてから描くまでの遅れ。音から分析の結果が届くまでの遅れは含まない",
      });
      let [b, a] = f0Row();
      await record("start-0st", near(b, 150, 1) && near(a, 150, 1), { before: b, after: a });

      setNumber("pitch-num", 5);
      await sleep(3000);
      [b, a] = f0Row();
      await record("B4-shift", near(b, 150, 1) && near(a, 150 * 2 ** (5 / 12), 1.5), { before: b, after: a });

      click("freeze-all");
      const held = [stateText("before"), stateText("after")];
      await sleep(1500);
      const still = [stateText("before"), stateText("after")];
      const saveEnabled = !disabled("save");
      await record("B5-freeze", frozen("before") && frozen("after") && JSON.stringify(held) === JSON.stringify(still), { held, still });

      // 止めている間に図の大きさが変わっても（窓の最小化や大きさの変更）、描いた点が残る
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

      click("freeze-after");
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

      // 決まった幅で上げ下げするボタンと、元に戻すボタン
      const value = (id: string) => q<HTMLInputElement>(`#${id}`).value;
      q<HTMLButtonElement>('button.reset[data-target="pitch"]').click();
      const afterReset = value("pitch-num");
      q<HTMLButtonElement>('button[data-target="pitch"][data-step="1"]').click();
      q<HTMLButtonElement>('button[data-target="pitch"][data-step="0.1"]').click();
      const afterSteps = value("pitch-num");
      q<HTMLButtonElement>('button[data-target="formant"][data-step="0.05"]').click();
      q<HTMLButtonElement>('button[data-target="formant"][data-step="-0.01"]').click();
      const formant = value("formant-num");
      q<HTMLButtonElement>('button.reset[data-target="formant"]').click();
      await record("step-buttons", afterReset === "0.0" && afterSteps === "1.1" && formant === "1.04" && value("formant-num") === "1.00", {
        afterReset,
        afterSteps,
        formant,
        formantReset: value("formant-num"),
      });

      // 出力とモニターに流す音の切り替え（1 つのボタンで両方。押すたびに入れ替わる。spec の D4）
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

      // 画面に収まり、縦に流さなくてよい
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
  } catch (e) {
    await record("error", false, { message: String(e) });
  }
  const passed = results.length > 0 && results.every((r) => r.result === "PASS");
  await invoke("finish_selftest", { report: JSON.stringify(results, null, 2), passed });
  console.log(dir);
}
