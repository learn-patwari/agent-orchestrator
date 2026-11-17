import { expect, test } from "@playwright/test";

test("red numbers sit on the tabs that need you, and the list opens the exact place", async ({ page }) => {
  await page.goto("/chat/");
  const nav = page.getByRole("navigation", { name: "Main" });
  await expect(nav.getByRole("link", { name: /Features/ }).getByRole("status")).toHaveText("1");
  await expect(nav.getByRole("link", { name: /Inbox/ }).getByRole("status")).toHaveText("1");
  await expect(nav.getByRole("link", { name: /^Agents/ }).getByRole("status")).toHaveText("1");
  await expect(nav.getByRole("link", { name: /Pipeline/ }).getByRole("status")).toHaveCount(0);

  await page.getByRole("button", { name: /Notifications/ }).click();
  const list = page.getByRole("region", { name: "Waiting list" });
  await expect(list.getByText("Which database?")).toBeVisible();
  await expect(list.getByText("tester failed: Run tests")).toBeVisible();

  await list.getByRole("link", { name: /Approve the SRS/ }).click();
  await expect(page).toHaveURL(/\/features\/\?id=f1/);
  await expect(page.getByRole("button", { name: "Approve SRS" })).toBeVisible(); // that feature is already open
});
