import { describe, expect, it } from "vitest";
import { stamp } from "../lib/stamp";

// `at` is seconds since 1970, as the server stores it
const at = (y: number, mo: number, d: number, h: number, mi: number) => new Date(y, mo - 1, d, h, mi).getTime() / 1000;

describe("stamp", () => {
  const now = new Date(2026, 10, 18, 16, 30);
  it("shows only the time for a message from today", () => {
    expect(stamp(at(2026, 11, 18, 9, 5), now)).toBe("09:05");
  });
  it("adds the day for an older message", () => {
    expect(stamp(at(2026, 11, 17, 22, 41), now)).toBe("17 Nov, 22:41");
  });
  it("adds the year for a message from another year", () => {
    expect(stamp(at(2025, 12, 31, 23, 59), now)).toBe("31 Dec 2025, 23:59");
  });
  it("shows nothing when the time is unknown", () => {
    expect(stamp(undefined, now)).toBe("");
    expect(stamp(NaN, now)).toBe("");
  });
});
