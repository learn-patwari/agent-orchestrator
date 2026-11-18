import { expect, test } from "@playwright/test";

test("every chat message shows when it was written", async ({ page }) => {
  const at = new Date(2026, 5, 3, 14, 5).getTime() / 1000; // an old date: the day is shown with the time
  await page.route("**/api/chat", (r) =>
    r.request().method() === "GET"
      ? r.fulfill({ json: [{ role: "user", content: "Start development", at }, { role: "assistant", content: "Started.", at: at + 60 }] })
      : r.fallback(),
  );
  await page.goto("/chat/");
  const log = page.getByRole("log", { name: "Conversation" });
  await expect(log.getByText("Start development")).toBeVisible();
  await expect(log.getByText("3 Jun, 14:05")).toBeVisible();
  await expect(log.getByText("3 Jun, 14:06")).toBeVisible();
});

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
