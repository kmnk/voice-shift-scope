import { describe, expect, it } from "vitest";
import { KEYS, setLang, t } from "./i18n";
import domainErrors from "../../crates/voice-domain/src/error.rs?raw";

describe("i18n", () => {
  it("has the same keys in English and Japanese", () => {
    expect([...KEYS.ja].sort()).toEqual([...KEYS.en].sort());
  });
  it("changes the text with the language and fills in values", () => {
    (globalThis as { document?: unknown }).document = { documentElement: { lang: "" } };
    setLang("en");
    expect(t("saved", { path: "a.png" })).toBe("Saved: a.png");
    setLang("ja");
    expect(t("saved", { path: "a.png" })).toBe("保存しました: a.png");
  });
  it("has text for every domain error and for the device and audio errors", () => {
    const variants = [...domainErrors.matchAll(/^\s{4}([A-Z]\w*)\b/gm)].map((m) => m[1]);
    expect(variants.length).toBeGreaterThan(15);
    for (const name of [...variants, "Devices", "Pipeline"]) expect(KEYS.en).toContain(`err.${name}`);
  });
});
