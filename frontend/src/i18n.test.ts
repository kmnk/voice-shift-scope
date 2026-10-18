import { describe, expect, it } from "vitest";
import { KEYS, setLang, t } from "./i18n";

describe("i18n", () => {
  it("英語と日本語が同じ鍵を持つ", () => {
    expect([...KEYS.ja].sort()).toEqual([...KEYS.en].sort());
  });
  it("言葉を替えると文が替わり、値を埋める", () => {
    (globalThis as { document?: unknown }).document = { documentElement: { lang: "" } };
    setLang("en");
    expect(t("saved", { path: "a.png" })).toBe("Saved: a.png");
    setLang("ja");
    expect(t("saved", { path: "a.png" })).toBe("保存しました: a.png");
  });
});
