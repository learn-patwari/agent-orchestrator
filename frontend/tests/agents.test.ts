import { describe, expect, it } from "vitest";
import { runningCount, shownRows } from "../lib/agents";
import { STATUS_COLORS, toStatus } from "../lib/office-scene";

const row = (role: string, state: string) => ({ role, state });

describe("toStatus", () => {
  it("maps the words the backend uses onto the four office states", () => {
    expect(toStatus("running")).toBe("running");
    expect(toStatus("done")).toBe("done");
    expect(toStatus("completed")).toBe("done");
    expect(toStatus("failed")).toBe("failed");
    expect(toStatus("error")).toBe("failed");
    expect(toStatus("held")).toBe("idle");
    expect(toStatus("idle")).toBe("idle");
  });
  it("has one colour per state, all different", () => {
    expect(new Set(Object.values(STATUS_COLORS)).size).toBe(4);
  });
});

describe("shownRows", () => {
  it("leaves rows alone before any check has run", () => {
    expect(shownRows([row("cicd", "failed")], null)).toEqual([row("cicd", "failed")]);
  });
  it("shows a failed run as ready when the model answers the check, and remembers the last run", () => {
    const out = shownRows([row("cicd", "failed")], { cicd: { responding: true } });
    expect(out[0]).toMatchObject({ state: "idle", lastRun: "failed" });
  });
  it("keeps a failed run red when the model does not answer", () => {
    expect(shownRows([row("cicd", "failed")], { cicd: { responding: false } })[0].state).toBe("failed");
  });
  it("never changes done, running or idle rows", () => {
    const rows = [row("a", "done"), row("b", "running"), row("c", "idle")];
    expect(shownRows(rows, { a: { responding: true }, b: { responding: true }, c: { responding: true } })).toEqual(rows);
  });
});

describe("runningCount", () => {
  it("counts only agents that are running, not done or failed ones", () => {
    expect(runningCount([row("a", "running"), row("b", "done"), row("c", "failed"), row("d", "idle"), row("e", "running")])).toBe(2);
    expect(runningCount([])).toBe(0);
  });
});
