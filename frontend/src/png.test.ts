import { describe, expect, it } from "vitest";
import { fileName } from "./png";

describe("fileName", () => {
  it("日時の入った名前にする", () => {
    expect(fileName(new Date(2026, 9, 18, 9, 5, 7))).toBe("voice-shift-scope-20261018-090507.png");
  });
});
