// 画面の通しの試験。tauri-driver（W3C WebDriver）でアプリを動かし、利用者と同じ画面の操作で確かめる。
// 入力は --input-wav の試験用の母音、設定と画像の保存先は一時的な場所（利用者のファイルに触れない）。
//
// 使い方: node scripts/ui-test.mjs --app <exe> --input-wav <wav> --out <dir> [--expect-no-cable]
// tauri-driver を先に 4444 番で動かしておく。結果は <dir>/ui-report.json と画面の画像。

import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]?.startsWith("--") ? true : all[i + 1] ?? true]] : acc), []),
);
const DRIVER = "http://127.0.0.1:4444";
const out = resolve(args.out ?? "ui-out");
const saveDir = join(out, "saved");
const configDir = join(out, "config");
mkdirSync(saveDir, { recursive: true });
mkdirSync(configDir, { recursive: true });

async function wd(method, path, body) {
  const res = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json();
  if (!res.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json)}`);
  return json.value;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function record(criterion, pass, detail) {
  results.push({ criterion, result: pass ? "PASS" : "FAIL", detail });
  console.log(`${criterion}\t${pass ? "PASS" : "FAIL"}\t${JSON.stringify(detail)}`);
}

const session = await wd("POST", "/session", {
  capabilities: {
    alwaysMatch: {
      "tauri:options": {
        application: resolve(args.app),
        args: args["input-wav"] ? ["--input-wav", resolve(args["input-wav"])] : [],
      },
    },
  },
});
const sid = session.sessionId;
const S = (p) => `/session/${sid}${p}`;
const run = (script, ...a) => wd("POST", S("/execute/sync"), { script, args: a });
const text = (testid) => run(`return document.querySelector('[data-testid="${testid}"]')?.textContent ?? null`);
const click = (testid) => run(`document.querySelector('[data-testid="${testid}"]').click()`);
const disabled = (testid) => run(`return document.querySelector('[data-testid="${testid}"]').disabled`);
const setNumber = (id, v) => run(`const e=document.getElementById(arguments[0]); e.value=String(arguments[1]); e.dispatchEvent(new Event('change'));`, id, v);
const stateText = (side) => run(`return document.getElementById(arguments[0] + '-state').textContent`, side);
/// 数値の表の F0 の行の、前と後の値
const f0Row = async () =>
  run(`const r=[...document.querySelectorAll('[data-testid="numbers"] tr')].find(t=>t.cells[0]?.textContent.startsWith('F0'));
       return r ? [r.cells[1].textContent, r.cells[2].textContent] : null`).then((r) => (r ? r.map(Number) : [NaN, NaN]));
const screenshot = async (name) => writeFileSync(join(out, name), Buffer.from(await wd("GET", S("/screenshot")), "base64"));
const near = (v, target, pct) => Math.abs(v / target - 1) * 100 <= pct;

try {
  await sleep(3000);
  const cable = await text("cable-status");
  if (args["expect-no-cable"]) {
    record("C4", cable.includes("見つかりません") && (await disabled("start")), { cable, startDisabled: await disabled("start") });
  } else {
    record("cable-found", cable.startsWith("出力の先"), { cable });
    await click("start");
    await sleep(4000);
    const stats = JSON.parse((await text("test-stats")) || "{}");
    record("B6", stats.drawsPerSecond >= 30 && stats.receiveToDrawMs <= 100, {
      ...stats,
      note: "受けてから描くまでの遅れ。音から分析の結果が届くまでの遅れは含まない",
    });
    let [b, a] = await f0Row();
    record("start-0st", near(b, 150, 1) && near(a, 150, 1), { before: b, after: a });

    await setNumber("pitch-num", 5);
    await sleep(2500);
    [b, a] = await f0Row();
    record("B4-shift", near(b, 150, 1) && near(a, 150 * 2 ** (5 / 12), 1.5), { before: b, after: a });
    await screenshot("live.png");

    // B5: 全体を止める
    await click("freeze-all");
    const frozenState = [await stateText("before"), await stateText("after")];
    await sleep(1500);
    const stillFrozen = [await stateText("before"), await stateText("after")];
    const saveEnabled = !(await disabled("save"));
    record("B5-freeze", frozenState.every((s) => s.startsWith("止めている")) && JSON.stringify(frozenState) === JSON.stringify(stillFrozen), {
      frozenState,
      stillFrozen,
    });

    // C5 の一部: 止めている間だけ保存できる
    await click("save");
    await sleep(3000);
    const saved = readdirSync(saveDir).filter((f) => f.endsWith(".png"));
    record("C5-save", saveEnabled && saved.length === 1, { saveEnabled, saved });

    await click("freeze-all");
    await sleep(500);
    record("B5-resume", (await stateText("before")) === "" && (await stateText("after")) === "" && (await disabled("save")), {});

    // B7: 変えた後だけを止めて、ピッチを替える
    await click("freeze-after");
    await setNumber("pitch-num", 7);
    await sleep(2500);
    [b, a] = await f0Row();
    const header = await run(`return document.querySelector('[data-testid="numbers"] tr').textContent`);
    record("B7", near(a, 150 * 2 ** (5 / 12), 1.5) && near(b, 150, 1) && header.includes("時刻が違う"), { before: b, after: a, header });
    await screenshot("after-frozen.png");

    // B8: 片方を止めた後の全体の操作
    await click("freeze-all");
    const both = [await stateText("before"), await stateText("after")];
    await click("freeze-all");
    const none = [await stateText("before"), await stateText("after")];
    record("B8", both.every((s) => s.startsWith("止めている")) && none.every((s) => s === ""), { both, none });

    await click("stop");
  }
} catch (e) {
  record("error", false, { message: String(e) });
} finally {
  await wd("DELETE", S("")).catch(() => {});
  writeFileSync(join(out, "ui-report.json"), JSON.stringify(results, null, 2));
}
process.exit(results.every((r) => r.result === "PASS") ? 0 : 1);
