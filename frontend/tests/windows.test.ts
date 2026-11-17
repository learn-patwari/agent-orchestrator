import { describe, expect, it } from "vitest";
import { noWindow, verdictClass, windowBody } from "../lib/windows";

describe("usage window form", () => {
  it("sends nothing when switched off or for OpenRouter", () => {
    expect(windowBody(noWindow, "openai")).toBeUndefined();
    expect(windowBody({ ...noWindow, enabled: true }, "openrouter")).toBeUndefined();
  });
  it("turns the typed values into the API body", () => {
    const b = windowBody({ enabled: true, hours: "5", start: "2026-01-01T09:00", ping: true, limit: "1000000" }, "anthropic")!;
    expect(b).toMatchObject({ enabled: true, length_hours: 5, ping: true, token_limit: 1000000 });
    expect(b.anchor).toBe(new Date("2026-01-01T09:00").getTime() / 1000);
  });
  it("falls back to 5 hours and no limit", () => {
    expect(windowBody({ ...noWindow, enabled: true, hours: "" }, "openai")).toMatchObject({ length_hours: 5, token_limit: undefined });
  });
  it("is green on a match, red on a difference, grey otherwise", () => {
    expect([verdictClass("match"), verdictClass("differ"), verdictClass("unknown")]).toEqual(["win-ok", "win-bad", "win-none"]);
  });
});
