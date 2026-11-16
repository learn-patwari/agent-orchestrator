import { expect, test } from "@playwright/test";

test("a feature waiting for SRS approval shows Approve and Reject right on the page, and the SRS as a real table", async ({ page }) => {
  await page.goto("/features/");
  await page.getByRole("button", { name: /notification feature/ }).click();
  const decision = page.getByRole("region", { name: "SRS approval" });
  await expect(decision.getByRole("button", { name: "Approve SRS" })).toBeVisible();
  await expect(decision.getByRole("button", { name: "Reject" })).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "Item" })).toBeVisible();
  await expect(page.getByRole("cell", { name: "Approved by" })).toBeVisible();
  await expect(page.locator("pre")).toHaveCount(0); // no raw markdown dump
});
