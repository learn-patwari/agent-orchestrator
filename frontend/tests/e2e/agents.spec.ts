import { expect, test } from "@playwright/test";

// Window sizes people actually have: a small laptop, a full-HD monitor, a large monitor.
const sizes = [
  { width: 1364, height: 720 },
  { width: 1920, height: 1080 },
  { width: 2560, height: 1440 },
  { width: 1920, height: 720 }, // wide but short: the height is what limits the office
];

for (const size of sizes) {
  test(`the Agents office fits one screen at ${size.width}x${size.height}`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.goto("/status/");
    const canvas = page.locator(".office-canvas");
    await expect(canvas).toBeVisible();
    const m = await page.evaluate(() => {
      const c = document.querySelector(".office-canvas")!.getBoundingClientRect();
      const side = document.querySelector(".office-side")!;
      return { docH: document.documentElement.scrollHeight, vh: innerHeight, canvasBottom: c.bottom, canvasRight: c.right, vw: innerWidth, sideScrolls: side.scrollHeight > side.clientHeight + 1 };
    });
    expect(m.docH, "the page must not scroll").toBeLessThanOrEqual(m.vh);
    expect(m.canvasBottom, "the office must end inside the window").toBeLessThanOrEqual(m.vh);
    expect(m.canvasRight).toBeLessThanOrEqual(m.vw);
    expect(m.sideScrolls, "the agent list must not need its own scrollbar").toBe(false);
  });
}

test("the office fills more than the old 1100px column on a big monitor", async ({ page }) => {
  await page.setViewportSize({ width: 2560, height: 1440 });
  await page.goto("/status/");
  const w = await page.locator(".office-canvas").evaluate((c) => c.getBoundingClientRect().width);
  expect(w).toBeGreaterThan(1100);
});

test("every agent shows the same state words in the list as the office uses", async ({ page }) => {
  await page.goto("/status/");
  const chips = page.locator(".agent-chip");
  await expect(chips).toHaveCount(9);
  await expect(chips.filter({ hasText: "failed" })).toHaveCount(4);
  await expect(chips.filter({ hasText: "done" })).toHaveCount(2);
  await expect(chips.filter({ hasText: "idle" })).toHaveCount(3);
});

test("the live strip says how many are working and shows the token use", async ({ page }) => {
  await page.goto("/status/");
  const strip = page.locator(".live-strip");
  await expect(strip).toContainText("0 of 9 working now");
  await expect(strip).toContainText("256.8k");
  await expect(strip).toContainText("Updates every 5 s");
});

test("the detached window shows only the office: no sidebar, no token bar, but the live strip", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 820 });
  await page.goto("/agents-window/");
  await expect(page.locator(".office-canvas")).toBeVisible();
  await expect(page.locator(".sidebar")).toBeHidden();
  await expect(page.locator(".tokenbar")).toBeHidden();
  await expect(page.locator(".live-strip")).toContainText("working now");
  const fits = await page.evaluate(() => document.documentElement.scrollHeight <= innerHeight);
  expect(fits, "the detached window must not scroll").toBe(true);
});
