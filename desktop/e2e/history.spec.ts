import fs from "node:fs";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures/app";
import type { AppFixture } from "./fixtures/app";

/**
 * History view (`desktop/src/lib/components/HistoryView.svelte`) end-to-end.
 *
 * The seed data comes from `buildHistory()` in `tauri-mock.js`:
 *  - 6 connections × 2 successful entries (one of them — prod-db-primary's
 *    first entry — is `{ Error: "Connection refused" }`, exit 255),
 *  - gpu-trainer "Active" (no ended_at) → still running,
 *  - staging-web "Terminated" with exit_code 0 → success-like (regression),
 *  - build-runner "Terminated" with exit_code 130 → failed,
 *  - homelab-nas "Disconnected" exit 0.
 * That is 16 rows: 13 succeeded, 2 failed, 1 running.
 */

const DATA_ROW = "tbody tr:has(td:nth-child(5))";

async function gotoHistory(page: Page, app: AppFixture) {
  await app.gotoApp();
  await app.nav("History");
  await expect(page.getByRole("heading", { name: "History" })).toBeVisible();
  // Wait until the table has all seeded rows so assertions aren't racing the load.
  await expect(page.locator(DATA_ROW)).toHaveCount(16);
}

test("statuses render the right labels (success, running, error)", async ({ page, app }) => {
  await gotoHistory(page, app);

  const rows = page.locator(DATA_ROW);

  // "Active" (no ended_at) is a live session, not a finished one.
  await expect(rows.filter({ hasText: "gpu-trainer" }).getByText("Running", { exact: true })).toBeVisible();

  // A plain "Success" history entry renders as "Succeeded".
  await expect(rows.filter({ hasText: "prod-api-01" }).first().getByText("Succeeded", { exact: true })).toBeVisible();

  // `{ Error: string }` renders a failure label plus the message.
  const errored = rows.filter({ hasText: "Connection refused" });
  await expect(errored.getByText("Failed", { exact: true })).toBeVisible();
  await expect(errored.getByText("Connection refused", { exact: true })).toBeVisible();
  await expect(errored).toContainText("prod-db-primary");
});

test("REGRESSION: 'Terminated' with exit_code 0 renders Succeeded, exit_code 130 renders Failed", async ({ page, app }) => {
  await gotoHistory(page, app);

  const rows = page.locator(DATA_ROW);

  // staging-web: status "Terminated", exit_code 0, duration [1080, 0] → success.
  const terminatedZero = page.getByTitle("Status: Terminated");
  await expect(terminatedZero).toHaveCount(1);
  await expect(terminatedZero).toBeVisible();
  await expect(terminatedZero).toHaveText("Succeeded");
  await expect(terminatedZero).not.toHaveText("Failed");

  // The same Terminated row carries the success duration, confirming identity.
  await expect(page.locator("tr:has(span[title='Status: Terminated']) td:nth-child(3)")).toHaveText("18m");

  // build-runner: status "Terminated", exit_code 130 → genuine failure.
  const terminatedNonZero = page.getByTitle("Exited with code 130");
  await expect(terminatedNonZero).toHaveCount(1);
  await expect(terminatedNonZero).toHaveText("Failed");

  // No staging-web row (the host of the exit-0 Terminated entry) is ever failed.
  const stagingRows = rows.filter({ hasText: "staging-web" });
  await expect(stagingRows).toHaveCount(3);
  await expect(stagingRows.getByText("Succeeded", { exact: true })).toHaveCount(3);
  await expect(stagingRows.getByText("Failed", { exact: true })).toHaveCount(0);
});

test("durations render as human-readable times and never NaN", async ({ page, app }) => {
  await gotoHistory(page, app);

  const durations = page.locator("tbody tr:has(td:nth-child(5)) td:nth-child(3)");
  await expect(durations).toHaveCount(16);

  const texts = await durations.allTextContents();
  for (const text of texts) {
    const value = text.trim();
    expect(value, `duration cell must not render NaN (got "${value}")`).not.toContain("NaN");
    expect(value).toMatch(/^(—|<\d+s|\d+s|\d+m( \d+s)?|\d+h( \d+m)?)$/);
  }

  // chrono `Duration` shapes map to expected human strings.
  await expect(page.locator("tr:has(span[title='Status: Terminated']) td:nth-child(3)")).toHaveText("18m"); // [1080, 0]
  await expect(page.locator("tr:has(span[title='Exited with code 130']) td:nth-child(3)")).toHaveText("43s"); // [42, 500000000]
  await expect(page.locator("tr:has(span[title='Status: Disconnected']) td:nth-child(3)")).toHaveText("1h 35m"); // [5700, 0]
});

test("search and status chips filter the rows", async ({ page, app }) => {
  await gotoHistory(page, app);

  const rows = page.locator(DATA_ROW);
  const header = page.locator(".view-header");

  // No filter: all 16 rows.
  await expect(header).toContainText("16");

  // Text search narrows by host.
  const search = page.getByLabel("Filter history");
  await search.fill("gpu");
  await expect(rows).toHaveCount(1);
  await expect(header).toContainText("1 of 16");

  // A search with no matches shows the empty state.
  await search.fill("no-such-host-zzz");
  await expect(rows).toHaveCount(0);
  await expect(page.getByText("No sessions match")).toBeVisible();

  // Clearing restores everything.
  await page.getByLabel("Clear filter").click();
  await expect(page.getByLabel("Filter history")).toHaveValue("");
  await expect(rows).toHaveCount(16);

  const chips = page.getByRole("group", { name: "Filter by status" });

  await chips.getByRole("button", { name: /^Failed/ }).click();
  await expect(rows).toHaveCount(2);
  await expect(header).toContainText("2 of 16");

  await chips.getByRole("button", { name: /^Succeeded/ }).click();
  await expect(rows).toHaveCount(13);
  await expect(header).toContainText("13 of 16");

  await chips.getByRole("button", { name: /^All/ }).click();
  await expect(rows).toHaveCount(16);
});

test("Export CSV downloads the session history", async ({ page, app }) => {
  await gotoHistory(page, app);

  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: /Export CSV/ }).click(),
  ]);

  expect(download.suggestedFilename()).toMatch(/^bayesian_ssh_history_\d+\.csv$/);

  const path = await download.path();
  expect(path).not.toBeNull();
  const csv = fs.readFileSync(path as string, "utf8");
  const lines = csv.trim().split("\n");

  expect(lines[0]).toBe("Connection,StartedAt,EndedAt,Status,ExitCode,DurationSec");
  // 16 history entries + header.
  expect(lines).toHaveLength(17);
  expect(csv).toContain('"staging-web"');
  expect(csv).toContain('"Connection refused"');
  // Terminated/exit-0 exports as a rounded duration, never NaN.
  expect(csv).not.toContain("NaN");
});
