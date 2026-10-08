import { test, expect } from "./fixtures/app";

// Sanity check that the fixture boots the real app against the IPC mock.
test("shell renders with hosts loaded", async ({ app, page }) => {
  await app.gotoApp();
  await expect(page.locator(".app-shell")).toBeVisible();
  await app.nav("Hosts");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Hosts");
  await expect(page.getByText("prod-api-01")).toBeVisible();
  const calls = await app.ipcCalls("get_connections");
  expect(calls.length).toBeGreaterThan(0);
});
