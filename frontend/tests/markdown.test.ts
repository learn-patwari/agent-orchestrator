import { describe, expect, it } from "vitest";
import { parseMarkdown, srsApprovalItem } from "../lib/markdown";

describe("parseMarkdown", () => {
  it("reads headings, paragraphs and bullet lists", () => {
    const b = parseMarkdown("## 11. Sign-off\n\nSome text\nmore text\n\n- one\n- two");
    expect(b).toEqual([
      { type: "heading", level: 2, text: "11. Sign-off" },
      { type: "para", text: "Some text more text" },
      { type: "list", items: ["one", "two"] },
    ]);
  });
  it("reads a pipe table, dropping the |---| separator row", () => {
    const b = parseMarkdown("| Item | Value |\n|---|---|\n| Approved by | |\n| Date | |");
    expect(b).toEqual([{ type: "table", head: ["Item", "Value"], rows: [["Approved by", ""], ["Date", ""]] }]);
  });
  it("does not turn markup into anything but text (the SRS is written by a model)", () => {
    const b = parseMarkdown("<script>alert(1)</script>");
    expect(b).toEqual([{ type: "para", text: "<script>alert(1)</script>" }]);
  });
  it("copes with a table row that wraps onto the next line", () => {
    const b = parseMarkdown("| Task | Files |\n|---|---|\n| P4-T1: Panel | a.tsx,\nb.tsx |");
    expect(b[0]).toMatchObject({ type: "table", rows: [["P4-T1: Panel", "a.tsx, b.tsx"]] });
  });
});

describe("srsApprovalItem", () => {
  const items = [
    { id: "1", kind: "question", feature_id: "f1" },
    { id: "2", kind: "approval", action: "srs_approval", feature_id: "f2", status: "open" },
    { id: "3", kind: "approval", action: "srs_approval", feature_id: "f1", status: "open" },
  ];
  it("finds the open SRS approval for one feature", () => {
    expect(srsApprovalItem(items, "f1")?.id).toBe("3");
    expect(srsApprovalItem(items, "nope")).toBeUndefined();
  });
});
