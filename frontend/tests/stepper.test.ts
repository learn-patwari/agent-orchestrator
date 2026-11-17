import { describe, expect, it } from "vitest";
import { sections, stepsFor } from "../lib/stepper";

describe("stepsFor", () => {
  it("marks earlier steps done, the current one, and later ones to do", () => {
    const s = stepsFor("questioning");
    expect(s.map((x) => x.id)).toEqual(["setup", "skills", "questions", "srs", "build"]);
    expect(s.map((x) => x.state)).toEqual(["done", "done", "current", "todo", "todo"]);
  });
  it("sits on the SRS step while the SRS waits for a decision, and finishes after approval", () => {
    expect(stepsFor("srs_draft").find((x) => x.state === "current")?.id).toBe("srs");
    expect(stepsFor("ready").find((x) => x.state === "current")?.id).toBe("questions");
    expect(stepsFor("approved").find((x) => x.state === "current")?.id).toBe("build");
  });
  it("shows a rejected SRS on the SRS step", () => {
    expect(stepsFor("rejected").find((x) => x.state === "current")?.id).toBe("srs");
  });
  it("an unknown status starts at the first step", () => {
    expect(stepsFor("???")[0].state).toBe("current");
  });
});

describe("sections", () => {
  it("splits the SRS at each top-level heading so each part can fold", () => {
    const out = sections("# Title\n\nintro\n\n## 1. Scope\n\ntext\n\n- a\n\n## 2. Risks\n\nmore");
    expect(out.map((x) => x.title)).toEqual(["Title", "1. Scope", "2. Risks"]);
    expect(out[1].blocks).toEqual([
      { type: "para", text: "text" },
      { type: "list", items: ["a"] },
    ]);
  });
  it("keeps text that comes before any heading in an untitled section", () => {
    expect(sections("hello")[0]).toMatchObject({ title: "" });
  });
  it("is empty for no text", () => {
    expect(sections("")).toEqual([]);
  });
});
