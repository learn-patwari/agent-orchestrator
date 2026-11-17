import { expect, test } from "@playwright/test";

test("a provider card shows its window, red when the provider's reset differs", async ({ page }) => {
  await page.goto("/providers/");
  const card = page.locator(".card", { hasText: "Anthropic" }).first();
  await expect(card.getByText(/Window 5 h/)).toBeVisible();
  await expect(card.locator(".win-bad")).toContainText("differs");
  await expect(card.getByRole("button", { name: "Check now" })).toBeVisible();
});

test("the add form asks for the window, except for OpenRouter", async ({ page }) => {
  await page.goto("/providers/");
  await page.locator("#pr").selectOption("openai");
  await page.getByLabel(/Keep a repeating usage window/).check();
  await expect(page.getByLabel("Window length (hours)")).toHaveValue("5");
  await expect(page.getByLabel(/Send a tiny message/)).toBeChecked();
  await page.locator("#pr").selectOption("openrouter");
  await expect(page.getByText("Usage window")).toHaveCount(0);
});
