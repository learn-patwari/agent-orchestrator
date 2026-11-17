import { expect, test } from "@playwright/test";

test("clicking an agent opens a chat with that agent only", async ({ page }) => {
  await page.route("**/api/agents/developer_1/chat**", (r) =>
    r.request().method() === "POST" ? r.fulfill({ json: { reply: "I changed the button.", session_id: "s1" } }) : r.fulfill({ json: [] }),
  );
  await page.goto("/status/");
  await page.getByRole("button", { name: /Dev 1|Developer 1|developer_1/i }).first().click();
  await page.getByRole("button", { name: /^Chat with/ }).click();
  const chat = page.getByRole("complementary", { name: /Chat with/ });
  await expect(chat.getByText(/cannot start other agents/)).toBeVisible();
  await chat.getByLabel(/Message to/).fill("what did you change?");
  await chat.getByRole("button", { name: "Send" }).click();
  await expect(chat.getByText("I changed the button.")).toBeVisible();
  await expect(chat.getByText("what did you change?")).toBeVisible();
});
