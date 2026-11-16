import { defineConfig } from "@playwright/test";

// Runs against the exported UI and a mock server. Uses the Edge that ships with Windows, so no browser is downloaded.
export default defineConfig({
  testDir: "tests/e2e",
  use: { baseURL: "http://127.0.0.1:8797", channel: process.env.AO_BROWSER ?? "msedge" },
  webServer: { command: "node tests/e2e/mock-server.mjs", url: "http://127.0.0.1:8797/status/", reuseExistingServer: !process.env.CI },
});
