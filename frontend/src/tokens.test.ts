import { describe, expect, it } from "vitest";
import styleCss from "./style.css?raw";

// S1: colour values live only in the :root variables of style.css
const sources = import.meta.glob("./*.ts", { query: "?raw", import: "default", eager: true });
const COLOUR = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\(\s*[0-9]/g;

/// style.css split into the :root block and everything else, without comments
function splitCss(): { root: string; rest: string } {
  const css = styleCss.replace(/\/\*[\s\S]*?\*\//g, "");
  const start = css.indexOf(":root {");
  const end = css.indexOf("}", start);
  return { root: css.slice(start, end), rest: css.slice(0, start) + css.slice(end + 1) };
}

describe("S1 design tokens", () => {
  it("has colours only in the :root variables of style.css", () => {
    expect(splitCss().rest.match(COLOUR) ?? []).toEqual([]);
  });

  it("defines the spacing, corner, and type variables in :root", () => {
    const { root } = splitCss();
    for (const name of ["--space-1", "--space-2", "--space-3", "--radius-s", "--radius-m", "--text-s", "--text-m", "--text-l"]) {
      expect(root).toContain(`${name}:`);
    }
  });

  it("has no hard-coded colours in the TypeScript sources", () => {
    const names = Object.keys(sources).filter((f) => !f.endsWith(".test.ts") && !f.endsWith(".d.ts"));
    expect(names).toContain("./render.ts");
    expect(names).toContain("./png.ts");
    const found = names.flatMap((f) => (sources[f].match(COLOUR) ?? []).map((m) => `${f}: ${m}`));
    expect(found).toEqual([]);
  });
});
