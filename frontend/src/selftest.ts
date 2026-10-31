// 画面の通しの試験（`--ui-selftest <dir>` で起動したときだけ動く）。
// 利用者と同じ操作（ボタンを押す・値を入れる）を DOM で順に行い、画面に出たものを読んで判定する。
// 言葉に依らないように、文ではなく data-* の印で状態を読む。結果は <dir>/ui-report.json に書き、アプリは終わる。

import { invoke } from "@tauri-apps/api/core";
import { lang, t } from "./i18n";
import { formatDb, formatKHz, gramFreqAt, spectrumDbAt, spectrumFreqAt, spectrumLayout } from "./render";

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
  // DOM を変えた直後は、まだ画面に描かれていない。描き直しを 2 回待ってから撮る
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
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
    // 文字は .label の span にある（アイコンの SVG を消さないよう、span だけを入れ替える）
    const label = b.querySelector<HTMLElement>(".label") ?? b;
    const keep = label.textContent;
    for (const text of texts) {
      label.textContent = text;
      const rect = b.getBoundingClientRect();
      const fits = b.scrollWidth <= b.clientWidth + 1 && rect.right <= groupRight && rect.height <= 25;
      out.push({ lang: lang(), id, text, fits, scrollWidth: b.scrollWidth, clientWidth: b.clientWidth, height: rect.height });
    }
    label.textContent = keep;
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


/// CSS の変数が実際に描かれる色（rgb(...) の形）
function resolved(name: string): string {
  const probe = document.createElement("span");
  probe.style.color = `var(--${name})`;
  document.body.append(probe);
  const c = getComputedStyle(probe).color;
  probe.remove();
  return c;
}

const rgbOf = (css: string) => (css.match(/\d+/g) ?? []).slice(0, 3).map(Number);

/// 文字と全体の表示のキーを、利用者が押したのと同じように送る（フォーカスのある要素から）
function press(key: string, opts: KeyboardEventInit = {}, target: Element = document.activeElement ?? document.body): void {
  target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...opts }));
}

function blur(): void {
  (document.activeElement as HTMLElement | null)?.blur?.();
}

/// 絵文字と記号のアイコン（spec の A3 で消したもの）
const OLD_ICONS = /[\u25B6\u25A0\u23F8\u21C4\u21BA\u21F2\u21F1\u26A0]|\u{1F3A7}|\u{1F4F7}/u;

/// S2: 見えているボタンに絵文字・記号のアイコンが無く、高さが 24px。アイコンを持つボタンは SVG を持つ
function checkButtons(): { lang: string; bad: unknown[]; checked: number } {
  const bad: unknown[] = [];
  let checked = 0;
  document.querySelectorAll<HTMLButtonElement>("button").forEach((b) => {
    if (b.offsetParent === null || b.classList.contains("link-button")) return;
    checked++;
    const h = Math.round(b.getBoundingClientRect().height);
    const text = b.textContent ?? "";
    const needsIcon = b.dataset.icon !== undefined || ["run", "route", "view-toggle"].includes(b.id);
    const hasSvg = b.querySelector("svg.icon") !== null;
    if (h !== 24 || OLD_ICONS.test(text) || (needsIcon && !hasSvg)) bad.push({ id: b.id || b.className, h, text, needsIcon, hasSvg });
  });
  const texts = [...document.querySelectorAll<HTMLElement>("figcaption, .message, .cable")].map((e) => e.textContent ?? "");
  for (const text of texts) if (OLD_ICONS.test(text)) bad.push({ text });
  return { lang: lang(), bad, checked };
}

/// S3: 図の題の凡例と、表の F1〜F4 の行の色の見本が、F1〜F4 の変数の色
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

/// S9: 止める欄のキーの札と、3 行の切り替えの並び
function checkHoldGrid(): { pass: boolean; detail: unknown } {
  const keys = [...document.querySelectorAll<HTMLElement>(".hold-row .key, .save .key")];
  const centers = keys.map((k) => {
    const r = k.getBoundingClientRect();
    return { text: k.textContent, center: r.left + r.width / 2 };
  });
  const switches = ["freeze-all", "freeze-before", "freeze-after"].map((id) => {
    const r = q(`[data-testid="${id}"]`).getBoundingClientRect();
    return { id, left: r.left, right: r.right, width: r.width };
  });
  const spread = (xs: number[]) => Math.max(...xs) - Math.min(...xs);
  const pass =
    centers.map((c) => c.text).join(",") === "0,1,2,S" &&
    spread(centers.map((c) => c.center)) <= 1 &&
    spread(switches.map((s) => s.left)) <= 0.5 &&
    spread(switches.map((s) => s.right)) <= 0.5;
  return { pass, detail: { centers, switches } };
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
    // DEVICES 欄のボタンの文言が、どちらの言葉でも 1 行に収まり、列からはみ出さない（spec の D5）
    const overflowing = [...fitOther, ...fitBack].filter((f) => !f.fits);
    await record("devices-fit", overflowing.length === 0, { overflowing, checked: fitOther.length + fitBack.length });
    // 絵文字・記号のアイコンが無く、どのボタンも高さ 24px（spec の S2）
    await record("S2-icons", buttonsOther.bad.length === 0 && buttonsBack.bad.length === 0 && buttonsOther.checked > 10, {
      other: buttonsOther,
      back: buttonsBack,
    });
    // F1〜F4 の凡例と表の色の見本（S3）、スペクトルの dB の目盛り（S6）、止める欄の並び（S9）
    const legend = checkLegend();
    await record("S3-legend", legend.pass, legend.detail);
    const dbTicksText = q<HTMLCanvasElement>("#spectrum").dataset.dbTicks;
    await record("S6-db-axis", dbTicksText === "0,-20,-40,-60,-80,-100", { dbTicks: dbTicksText });
    const grid = checkHoldGrid();
    await record("S9-hold-keys", grid.pass, grid.detail);

    // キー操作の一覧: ? で開き、開いている間は他のキーが効かず、Esc で閉じる（S12）
    blur();
    const dialog = q<HTMLDialogElement>('[data-testid="shortcuts"]');
    const pitchBeforeDialog = q<HTMLInputElement>("#pitch-num").value;
    const frozenBeforeDialog = [frozen("before"), frozen("after")];
    press("?");
    await sleep(300);
    const opened = dialog.open;
    const focusOnClose = document.activeElement?.id === "shortcuts-close";
    await shot("S12-shortcuts");
    for (const key of ["0", " ", "1", "2", "s", "ArrowUp", "ArrowRight"]) press(key);
    await sleep(200);
    const unchanged = q<HTMLInputElement>("#pitch-num").value === pitchBeforeDialog && frozen("before") === frozenBeforeDialog[0] && frozen("after") === frozenBeforeDialog[1];
    press("Escape");
    await sleep(200);
    const closed = !dialog.open;
    click("shortcuts-open");
    await sleep(200);
    const openedByButton = dialog.open;
    press("?");
    await sleep(200);
    await record("S12", opened && focusOnClose && unchanged && closed && openedByButton && !dialog.open, {
      opened,
      focusOnClose,
      unchanged,
      closed,
      openedByButton,
      closedByQuestion: !dialog.open,
    });

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

      const panelSize = () => ["before", "after"].map((s) => q(`#${s}-panel`).getBoundingClientRect()).map((r) => [r.width, r.height]);
      const sizeLive = panelSize();
      click("freeze-all");
      const held = [stateText("before"), stateText("after")];
      await sleep(1500);
      const still = [stateText("before"), stateText("after")];
      const saveEnabled = !disabled("save");
      await record("B5-freeze", frozen("before") && frozen("after") && JSON.stringify(held) === JSON.stringify(still), { held, still });

      // 止めた図: 2px の紫の枠、札、切り替えの HOLD。図の大きさは変わらない（S8）
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

      // 右端の値が数値の表の値と同じ（S4）
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

      // 倍音の点が --harmonic の色で、高さが 1px（拡大率を掛けた画素）（S5）
      {
        const cv = q<HTMLCanvasElement>("#before-gram");
        const [hr, hg, hb] = rgbOf(resolved("harmonic"));
        const data = cv.getContext("2d")!.getImageData(0, 0, cv.width, cv.height).data;
        const dpr = window.devicePixelRatio || 1;
        // 値の欄の手前の、最新の列を縦に見る
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

      // 図の上のカーソル: 周波数（スペクトルでは dB も）の札がマウスの位置の値を出し、外すと消える（S7）
      {
        const axisMax = Number(q<HTMLInputElement>("#ceiling").value) * Math.max(1, Number(q<HTMLInputElement>("#formant-num").value));
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
          { axisMax, gramText, gramWant, spText, spWant, cleared, note: "PNG は図の canvas だけから作り、カーソルを描く重ねた canvas を使わない（png.ts の composePng）" },
        );
      }

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
      const liveLook = ["before", "after"].map((side) => ({ held: q(`#${side}-panel`).dataset.held, badge: q(`#${side}-state`).textContent }));
      await record(
        "S8-resume",
        liveLook.every((l) => l.held === "false" && l.badge === "") && ["freeze-all", "freeze-before", "freeze-after"].every((id) => !pressed(id)),
        { liveLook },
      );

      click("freeze-after");
      // 片側だけ止めても、札の分で題の行が高くならず、左右の図の高さ（同じ高さが同じ周波数）がそろう（S8）
      {
        const gramHeights = ["before", "after"].map((side) => q<HTMLCanvasElement>(`#${side}-gram`).getBoundingClientRect().height);
        const held = ["before", "after"].map((side) => q(`#${side}-panel`).dataset.held);
        await record("S8-one-side", gramHeights[0] === gramHeights[1] && held[0] === "false" && held[1] === "true", { gramHeights, held });
      }
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

      // 矢印のキーでピッチとフォルマントが決めた幅だけ変わる。数の欄に入力しているときは変わらない（S10）
      {
        blur();
        const num = (id: string) => Number(q<HTMLInputElement>(`#${id}`).value);
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
        const sliderMatches = q<HTMLInputElement>("#formant").value === q<HTMLInputElement>("#formant-num").value;
        press("ArrowLeft", { shiftKey: true });
        press("ArrowLeft");
        const f3 = num("formant-num");
        // 数の欄にフォーカスがあるときは効かない
        const field = q<HTMLInputElement>("#pitch-num");
        field.focus();
        press("ArrowUp", {}, field);
        const inField = num("pitch-num");
        blur();
        const close = (a: number, b: number) => Math.abs(a - b) < 1e-6;
        await record(
          "S10-arrows",
          close(p1, p0 + 0.1) && close(p2, p0 + 1.1) && close(p3, p0) && close(f1, f0 + 0.01) && close(f2, f0 + 0.06) && close(f3, f0) && sliderMatches && close(inField, p0),
          { p0, p1, p2, p3, f0, f1, f2, f3, sliderMatches, inField },
        );
      }

      // 0 で全体を止め、S で保存、Space で戻す。止めていないときの S は何もしない（S9・S11）
      {
        blur();
        q("#message").textContent = "";
        q("#message").dataset.kind = "";
        press("0");
        await sleep(200);
        const heldByZero = frozen("before") && frozen("after");
        press("s");
        const savedByKey = await waitFor(() => q("#message").dataset.kind === "saved", 10_000);
        press(" ");
        await sleep(200);
        const resumedBySpace = !frozen("before") && !frozen("after");
        q("#message").textContent = "";
        q("#message").dataset.kind = "";
        press("S");
        await sleep(1500);
        const ignoredWhenLive = q("#message").dataset.kind !== "saved";
        await record("S11-save-key", heldByZero && savedByKey && resumedBySpace && ignoredWhenLive, {
          heldByZero,
          savedByKey,
          resumedBySpace,
          ignoredWhenLive,
        });
      }

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
