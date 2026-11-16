import { describe, expect, it } from "vitest";
import { actionWord, tabCounts, tabFor, type AttentionItem } from "../lib/attention";

const item = (kind: AttentionItem["kind"], link: string): AttentionItem => ({ id: link + kind, kind, title: "t", detail: "", link, at: 1 });

describe("attention", () => {
  it("puts each item on the tab its link goes to", () => {
    expect(tabFor("/features/?id=f1")).toBe("/features");
    expect(tabFor("/inbox/")).toBe("/inbox");
    expect(tabFor("/status/")).toBe("/status");
    expect(tabFor("/other")).toBe("/");
  });
  it("counts items per tab", () => {
    const c = tabCounts([item("approval", "/features/?id=f1"), item("question", "/features/?id=f2"), item("approval", "/inbox/"), item("task_failed", "/status/")]);
    expect(c).toEqual({ "/features": 2, "/inbox": 1, "/status": 1 });
  });
  it("says what to do", () => {
    expect(actionWord("approval")).toBe("Approve or reject");
    expect(actionWord("task_failed")).toBe("Look at the failure");
  });
});
