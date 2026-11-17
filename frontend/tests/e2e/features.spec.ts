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

test("a feature page walks through steps, shows a skill's description once it is ticked, and folds the SRS", async ({ page }) => {
  await page.goto("/features/");
  await page.getByRole("button", { name: /notification feature/ }).click();
  const steps = page.getByRole("list", { name: "Steps" });
  await expect(steps.getByRole("button", { name: /SRS/ })).toHaveAttribute("aria-current", "step"); // a draft SRS waits for you
  await steps.getByRole("button", { name: /Folder, model and limit/ }).click();
  await expect(page.getByLabel("Code folder for this feature")).toBeVisible();
  await expect(page.getByRole("button", { name: "Build graph" })).toBeVisible();
  await steps.getByRole("button", { name: /Skills/ }).click();
  await expect(page.getByText("How we use Postgres")).toHaveCount(0);
  await page.getByRole("checkbox", { name: /postgres-tips/ }).check();
  await expect(page.getByText("How we use Postgres")).toBeVisible();
  await expect(page.getByText(/Need a skill that is not listed/)).toBeVisible();
  await steps.getByRole("button", { name: /SRS/ }).click();
  await page.getByRole("button", { name: "Collapse all" }).click();
  await expect(page.getByRole("cell", { name: "Approved by" })).toBeHidden();
  await page.getByRole("button", { name: "Expand all" }).click();
  await expect(page.getByRole("cell", { name: "Approved by" })).toBeVisible();
});
