import { expect, test } from "@playwright/test";

test("the chat shows what waits for you and lets you approve it right there", async ({ page }) => {
  await page.goto("/chat/");
  const waiting = page.getByRole("region", { name: "Waiting for you" });
  await expect(waiting.getByText("Approve the SRS")).toBeVisible();
  const [req] = await Promise.all([
    page.waitForRequest((r) => r.url().endsWith("/api/inbox/i1/decide") && r.method() === "POST"),
    waiting.getByRole("button", { name: "Approve" }).click(),
  ]);
  expect(req.postDataJSON()).toEqual({ approve: true });
});
