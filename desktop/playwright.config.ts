import { defineConfig, devices } from "@playwright/test";

/**
 * End-to-end tests for the desktop SPA. They run the real Vite dev server in a
 * browser and stub the Tauri IPC layer with `e2e/fixtures/tauri-mock.js`.
 *
 * `npm run dev` is already expected to own port 1420; `reuseExistingServer`
 * keeps a running instance and CI starts one automatically.
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : undefined,
  timeout: 30_000,
  expect: { timeout: 7_000 },
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : [["list"]],
  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    viewport: { width: 1440, height: 900 },
  },
  webServer: {
    command: "npm run dev",
    port: 1420,
    reuseExistingServer: true,
    timeout: 120_000,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } },
    },
  ],
});
