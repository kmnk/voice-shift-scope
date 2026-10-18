// 画面の通しの試験（`--ui-selftest <dir>` で起動したときだけ動く）。
// 利用者と同じ操作（ボタンを押す・値を入れる）を DOM で順に行い、画面に出たものを読んで判定する。
// 言葉に依らないように、文ではなく data-* の印で状態を読む。結果は <dir>/ui-report.json に書き、アプリは終わる。

import { invoke } from "@tauri-apps/api/core";

type Result = { criterion: string; result: "PASS" | "FAIL"; detail: unknown };

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const q = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;
const click = (testid: string) => q<HTMLButtonElement>(`[data-testid="${testid}"]`).click();
const disabled = (testid: string) => q<HTMLButtonElement>(`[data-testid="${testid}"]`).disabled;
const frozen = (side: string) => q(`#${side}-state`).dataset.frozen === "true";
const stateText = (side: string) => q(`#${side}-state`).textContent ?? "";
const near = (v: number, target: number, pct: number) => Math.abs(v / target - 1) * 100 <= pct;
const pressed = (testid: string) => q(`[data-testid="${testid}"]`).getAttribute("aria-pressed") === "true";

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

export async function runSelftest(dir: string): Promise<void> {
  const results: Result[] = [];
  const record = (criterion: string, pass: boolean, detail: unknown) =>
    results.push({ criterion, result: pass ? "PASS" : "FAIL", detail });
  try {
    await waitFor(() => q("#cable").dataset.found !== undefined, 10_000);
    // 言葉の切り替え: 文が替わり、もう一度で戻る（選んだ言葉は試験の設定のディレクトリに覚える）
    const startEn = q("#start").textContent;
    const langBefore = document.documentElement.lang;
    click("lang-toggle");
    await sleep(200);
    const startOther = q("#start").textContent;
    const langOther = document.documentElement.lang;
    click("lang-toggle");
    await sleep(200);
    record("language-toggle", langBefore !== langOther && startEn !== startOther && q("#start").textContent === startEn, {
      langBefore,
      langOther,
      startEn,
      startOther,
    });
    // 簡易表示: 右の表示と分析・一時停止の欄が消え、もう一度で戻る
    click("view-toggle");
    await sleep(500);
    const compactHidden = getComputedStyle(q("#screen")).display === "none";
    const saveHidden = q<HTMLElement>("#save").offsetParent === null;
    click("view-toggle");
    await sleep(800);
    record("compact-view", compactHidden && saveHidden && getComputedStyle(q("#screen")).display !== "none", { compactHidden, saveHidden });

    if (q("#cable").dataset.found === "false") {
      // VB-CABLE が無いときは、出力が空で開始できず、知らせが出る
      record("C4", disabled("start") && q<HTMLSelectElement>("#output").value === "" && (q("#cable").textContent ?? "") !== "", {
        cable: q("#cable").textContent,
        startDisabled: disabled("start"),
      });
    } else {
      const out = q<HTMLSelectElement>("#output");
      record("cable-found", out.value !== "" && (q("#cable").textContent ?? "") === "", { output: out.selectedOptions[0]?.textContent, note: q("#cable").textContent });
      click("start");
      await sleep(5000);
      const stats = JSON.parse(q("#test-stats").textContent || "{}");
      record("B6", stats.drawsPerSecond >= 30 && stats.receiveToDrawMs <= 100, {
        ...stats,
        note: "受けてから描くまでの遅れ。音から分析の結果が届くまでの遅れは含まない",
      });
      let [b, a] = f0Row();
      record("start-0st", near(b, 150, 1) && near(a, 150, 1), { before: b, after: a });

      setNumber("pitch-num", 5);
      await sleep(3000);
      [b, a] = f0Row();
      record("B4-shift", near(b, 150, 1) && near(a, 150 * 2 ** (5 / 12), 1.5), { before: b, after: a });

      click("freeze-all");
      const held = [stateText("before"), stateText("after")];
      await sleep(1500);
      const still = [stateText("before"), stateText("after")];
      const saveEnabled = !disabled("save");
      record("B5-freeze", frozen("before") && frozen("after") && JSON.stringify(held) === JSON.stringify(still), { held, still });

      click("save");
      const saved = await waitFor(() => q("#message").dataset.kind === "saved", 10_000);
      record("C5-save", saveEnabled && saved, { saveEnabled, message: q("#message").textContent });

      click("freeze-all");
      await sleep(300);
      record("B5-resume", !frozen("before") && !frozen("after") && disabled("save"), {});

      click("freeze-after");
      setNumber("pitch-num", 7);
      await sleep(3000);
      [b, a] = f0Row();
      const differ = q("#times-differ").dataset.differ === "true";
      record("B7", near(a, 150 * 2 ** (5 / 12), 1.5) && near(b, 150, 1) && differ, { before: b, after: a, differ });

      click("freeze-all");
      const both = frozen("before") && frozen("after");
      click("freeze-all");
      const none = !frozen("before") && !frozen("after");
      record("B8", both && none, { both, none });

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
      record("step-buttons", afterReset === "0.0" && afterSteps === "1.1" && formant === "1.04" && value("formant-num") === "1.00", {
        afterReset,
        afterSteps,
        formant,
        formantReset: value("formant-num"),
      });

      // 出力とモニターに流す音の切り替え（続けて押しても前の押した分を失わない）
      click("output-original");
      await sleep(300);
      const outOriginal = pressed("output-original");
      click("monitor-original");
      await sleep(300);
      const monOriginal = pressed("monitor-original");
      click("output-shifted");
      click("monitor-shifted");
      await sleep(800);
      record("routing", outOriginal && monOriginal && pressed("output-shifted") && pressed("monitor-shifted"), {
        outOriginal,
        monOriginal,
        message: q("#message").textContent,
      });

      // 画面に収まり、縦に流さなくてよい
      const root = document.documentElement;
      record("fits-window", root.scrollHeight <= window.innerHeight + 1, { scrollHeight: root.scrollHeight, innerHeight: window.innerHeight });

      click("stop");
      await sleep(500);
    }
  } catch (e) {
    record("error", false, { message: String(e) });
  }
  const passed = results.length > 0 && results.every((r) => r.result === "PASS");
  await invoke("finish_selftest", { report: JSON.stringify(results, null, 2), passed });
  console.log(dir);
}
