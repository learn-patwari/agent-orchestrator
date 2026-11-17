import { describe, expect, it } from "vitest";
import { activityFor, short } from "../lib/activity";

const run = (o: object) => ({ id: "r", role: "architect", task: "t", status: "done", feature_id: "f1", ...o });

describe("activityFor", () => {
  it("names who is working on this feature and ignores other features", () => {
    const a = activityFor("f1", [run({ status: "running", role: "developer_1" }), run({ status: "running", feature_id: "f2" })], []);
    expect(a.working).toHaveLength(1);
    expect(a.headline).toBe("1 agent working now: developer_1.");
    expect(a.idle).toBe(false);
  });
  it("says plainly when nothing runs, and whether tasks wait", () => {
    expect(activityFor("f1", [], []).headline).toBe("Nothing has started for this feature yet.");
    expect(activityFor("f1", [run({})], []).headline).toContain("Nothing has been handed to a developer");
    expect(activityFor("f1", [run({})], [{ id: "t", name: "x", status: "open", feature_id: "f1" }]).headline).toContain("tasks below are waiting");
  });
  it("lists the latest finished runs, newest first", () => {
    const runs = [1, 2, 3, 4, 5, 6].map((n) => run({ id: `r${n}` }));
    expect(activityFor("f1", runs, []).recent.map((r) => r.id)).toEqual(["r6", "r5", "r4", "r3", "r2"]);
  });
});

describe("short", () => {
  it("cuts a long task at a word with an ellipsis", () => {
    const s = short("Brief for the architect agent Task: Your last report was cut off again, so I cannot see the lanes", 50);
    expect(s.endsWith("…")).toBe(true);
    expect(s.length).toBeLessThanOrEqual(51);
  });
  it("leaves short text alone", () => expect(short("hello")).toBe("hello"));
});
