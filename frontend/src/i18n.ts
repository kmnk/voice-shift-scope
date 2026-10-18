// 画面・画像・警告の言葉。日本語と英語を切り替える。アイコンはどちらの言葉でも同じものを使う。
// 選んだ言葉は preferences.json に覚える（初めて起動したときは英語）。

export type Lang = "en" | "ja";

const dict = {
  en: {
    "lang.toggle": "日本語",
    "lang.toggle.title": "Switch to Japanese",
    "sec.devices": "DEVICES",
    "sec.pitch": "PITCH (st)",
    "sec.formant": "FORMANT (×)",
    "sec.analysis": "ANALYSIS",
    "sec.monitor": "MONITOR",
    "sec.hold": "HOLD",
    "input": "IN",
    "start": "▶ START",
    "stop": "■ STOP",
    "output": "OUT",
    "route.output": "OUT",
    "route.monitor": "MON",
    "original": "ORIGINAL",
    "shifted": "SHIFTED",
    "reset": "↺",
    "reset.title.pitch": "Reset to 0 st",
    "reset.title.formant": "Reset to ×1.00",
    "ceiling": "Ceiling (Hz)",
    "ceiling.note": "Shifted side: × formant → {hz} Hz",
    "harm.floor": "Harm. floor + (dB)",
    "harm.range": "Harm. range (dB)",
    "harm.floor.title": "A harmonic counts when its peak is at least this much above the local noise floor",
    "harm.range.title": "…and within this many dB of the strongest harmonic",
    "monitor.on": "🎧 On",
    "volume": "VOL",
    "hold.all": "⏸ ALL",
    "resume.all": "▶ ALL",
    "hold.side": "⏸ {side}",
    "resume.side": "▶ {side}",
    "hold.title.all": "Hold or resume both sides (Space)",
    "hold.title.original": "Hold or resume the original side (1)",
    "hold.title.shifted": "Hold or resume the shifted side (2)",
    "snapshot": "📷 SNAPSHOT",
    "snapshot.title": "Save the held screen as PNG",
    "state.held": "HOLD {t} s",
    "panel.spectrum": "SPECTRUM — ORIGINAL (gray, dashed) / SHIFTED (blue)",
    "panel.values": "VALUES (250 ms median)",
    "times.differ": "⚠ times differ",
    "row.f0": "F0 (Hz)",
    "row.formant": "F{n} Hz (BW)",
    "row.observed": "Harmonics seen",
    "unselected": "(none)",
        "cable.missing": "VB-CABLE not found (https://vb-audio.com/Cable/). Choose another output, or install it and restart.",
    "saved": "Saved: {path}",
    "dropouts": "Dropouts: {parts}. If this continues, try another device or close other apps.",
    "dropouts.in": "IN {n} (last at {s} s)",
    "dropouts.out": "OUT {n}",
    "dropouts.mon": "MON {n}",
    "png.title": "voice-shift-scope   saved {date}",
    "png.side.none": "{label}: —",
    "png.side": "{label}: {state} · {t} s · pitch {st} st · formant ×{fm} · ceiling {ceil} Hz (shifted {aceil} Hz) · harmonics floor +{th} dB, range {rl} dB",
    "png.held": "HOLD",
    "png.live": "LIVE",
    "err.rule": "Not allowed: {msg}",
    "err.NoOutputDevice": "Choose an output device first.",
    "err.OutputChangeWhileRunning": "Stop before changing the output device.",
    "err.NoInputDevice": "Choose an input device first.",
    "err.NoMonitorDevice": "Choose a monitor device to turn monitoring on.",
    "err.InputChangeWhileRunning": "Stop before changing the input device.",
    "err.CableRecordingAsInput": "VB-CABLE's recording side cannot be the input.",
    "err.MonitorSameAsOutput": "The monitor cannot be the same device as the output.",
  },
  ja: {
    "lang.toggle": "EN",
    "lang.toggle.title": "英語にする",
    "sec.devices": "機器",
    "sec.pitch": "ピッチ (st)",
    "sec.formant": "フォルマント (×)",
    "sec.analysis": "分析",
    "sec.monitor": "モニター",
    "sec.hold": "一時停止",
    "input": "入力",
    "start": "▶ 開始",
    "stop": "■ 停止",
    "output": "出力",
    "route.output": "出力",
    "route.monitor": "モニター",
    "original": "元",
    "shifted": "変換",
    "reset": "↺",
    "reset.title.pitch": "0 st に戻す",
    "reset.title.formant": "×1.00 に戻す",
    "ceiling": "上限 (Hz)",
    "ceiling.note": "変換側: × フォルマント → {hz} Hz",
    "harm.floor": "倍音 床から + (dB)",
    "harm.range": "倍音 最大から (dB)",
    "harm.floor.title": "倍音の山が、まわりの雑音の床よりこれだけ高いときに数える",
    "harm.range.title": "さらに、最も強い倍音からこの dB 以内のときに数える",
    "monitor.on": "🎧 聞く",
    "volume": "音量",
    "hold.all": "⏸ 全体",
    "resume.all": "▶ 全体",
    "hold.side": "⏸ {side}",
    "resume.side": "▶ {side}",
    "hold.title.all": "両方を止める・戻す（Space）",
    "hold.title.original": "元の側を止める・戻す（1）",
    "hold.title.shifted": "変換の側を止める・戻す（2）",
    "snapshot": "📷 保存",
    "snapshot.title": "止めた画面を PNG で保存する",
    "state.held": "停止中 {t} s",
    "panel.spectrum": "スペクトル — 元（灰の破線）/ 変換（青）",
    "panel.values": "数値（250 ms の中央値）",
    "times.differ": "⚠ 時刻が違う",
    "row.f0": "F0 (Hz)",
    "row.formant": "F{n} Hz（帯域幅）",
    "row.observed": "観測できた倍音",
    "unselected": "（選んでいない）",
        "cable.missing": "VB-CABLE が見つかりません（https://vb-audio.com/Cable/ ）。別の出力を選ぶか、入れてから起動し直してください。",
    "saved": "保存しました: {path}",
    "dropouts": "音の取りこぼし: {parts}。続くときは別の機器を選ぶか、他のアプリを閉じてください。",
    "dropouts.in": "入力 {n} 回（最後は {s} 秒）",
    "dropouts.out": "出力 {n} 回",
    "dropouts.mon": "モニター {n} 回",
    "png.title": "voice-shift-scope　保存した日時 {date}",
    "png.side.none": "{label}: —",
    "png.side": "{label}: {state}・{t} 秒・ピッチ {st} st・フォルマント ×{fm}・上限 {ceil} Hz（変換 {aceil} Hz）・倍音 床から +{th} dB、最大から {rl} dB 以内",
    "png.held": "停止中",
    "png.live": "リアルタイム",
    "err.rule": "できません: {msg}",
    "err.NoOutputDevice": "出力の機器を選んでください。",
    "err.OutputChangeWhileRunning": "出力の機器は止めてから替えてください。",
    "err.NoInputDevice": "入力の機器を選んでください。",
    "err.NoMonitorDevice": "モニターを入れるには、モニターの機器を選んでください。",
    "err.InputChangeWhileRunning": "入力の機器は止めてから替えてください。",
    "err.CableRecordingAsInput": "VB-CABLE の録音側は入力に選べません。",
    "err.MonitorSameAsOutput": "モニターに、出力と同じ機器は選べません。",
  },
} as const;

export type Key = keyof (typeof dict)["en"];

let current: Lang = "en";

export function lang(): Lang {
  return current;
}

export function setLang(l: Lang): void {
  current = l;
  document.documentElement.lang = l;
}

export function t(key: Key, params: Record<string, string | number> = {}): string {
  let s: string = dict[current][key] ?? dict.en[key] ?? key;
  for (const [k, v] of Object.entries(params)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
}

export function hasKey(key: string): key is Key {
  return key in dict.en;
}

/// data-i18n（文字）と data-i18n-title（重ねると出る説明）の付いた要素を、今の言葉で書き直す。
export function applyStatic(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    const key = el.dataset.i18n!;
    if (hasKey(key)) el.textContent = t(key);
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-title]").forEach((el) => {
    const key = el.dataset.i18nTitle!;
    if (hasKey(key)) el.title = t(key);
  });
}

/// 英語と日本語で同じ鍵を持つこと（試験で確かめる）。
export const KEYS = { en: Object.keys(dict.en), ja: Object.keys(dict.ja) };
