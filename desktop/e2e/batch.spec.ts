import { test, expect, type Page } from "./fixtures/app";

/**
 * Batch execution modal (`BatchExecModal.svelte`).
 *
 * The component invokes `run_batch_command` with camelCase arguments
 * (`connectionIds`, `command`, `dryRun`, `timeoutSecs`) — the mock records the
 * arg object verbatim, so these tests pin the exact payload. The mock's backend
 * answers a dry run for every host and fails `build-runner` only on a live run.
 */

const ALL_IDS = ["id-0", "id-1", "id-2", "id-3", "id-4", "id-5", "id-6", "id-7"];
const HOST_COUNT = 8;

const dialog = (page: Page) => page.getByRole("dialog", { name: "Run command on hosts" });

/** The expandable result row for one host, inside the results table. */
const resultRow = (page: Page, name: string) =>
  dialog(page).locator(".table-wrap > div").filter({ hasText: name });

async function openFromHosts(page: Page) {
  await page.locator(".view-header").getByRole("button", { name: "Batch run" }).click();
  await expect(dialog(page)).toBeVisible();
}

test.describe("batch exec modal", () => {
  test("opens from the Hosts view with every host preselected", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);

    const modal = dialog(page);
    await expect(modal.getByText("Run one command on several hosts in parallel.")).toBeVisible();
    await expect(modal.getByText(`${HOST_COUNT} of ${HOST_COUNT} selected`)).toBeVisible();
    await expect(modal.locator("aside").getByRole("checkbox")).toHaveCount(HOST_COUNT);
    await expect(page.locator("#exec-command-input")).toHaveValue("uptime");
    await expect(modal.getByText("No results yet")).toBeVisible();

    // Dry run is on by default, so the primary action is a preview.
    await expect(modal.getByRole("checkbox", { name: "Dry run" })).toBeChecked();
    await expect(modal.getByRole("button", { name: `Preview on ${HOST_COUNT} hosts` })).toBeEnabled();
  });

  test("opens from the command palette", async ({ app, page }) => {
    await app.gotoApp();
    await app.openPalette();

    const palette = page.getByRole("dialog", { name: "Command palette" });
    await palette.getByRole("combobox", { name: "Search commands" }).fill("Run command on hosts");
    await page.keyboard.press("Enter");

    await expect(palette).toBeHidden();
    await expect(dialog(page)).toBeVisible();
  });

  test("a dry run posts the exact payload and previews every host", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);

    await page.locator("#exec-command-input").fill("df -h");
    await page.locator('input[aria-label="Timeout in seconds"]').fill("25");
    await app.clearIpc();

    await page.getByRole("button", { name: `Preview on ${HOST_COUNT} hosts` }).click();

    await app.waitForIpc("run_batch_command", 1);
    const calls = await app.ipcCalls("run_batch_command");
    expect(calls).toHaveLength(1);
    expect(calls[0]).toEqual({
      connectionIds: ALL_IDS,
      command: "df -h",
      dryRun: true,
      timeoutSecs: 25,
    });

    // One result row per host, all exit 0 in a dry run.
    await expect(dialog(page).locator(".table-wrap > div")).toHaveCount(HOST_COUNT);
    await expect(page.getByText(`${HOST_COUNT} passed`)).toBeVisible();
    await expect(dialog(page).getByText(/\d+ failed/)).toHaveCount(0);

    // Results replace the "No results yet" empty state.
    await expect(page.getByText("No results yet")).toHaveCount(0);

    // Expanded rows reveal the per-host stdout of the dry run.
    const api = resultRow(page, "prod-api-01");
    await api.locator("button[aria-expanded]").click();
    await expect(api.locator("pre")).toContainText("[dry-run] would run on deploy@10.0.4.12: df -h");

    const runner = resultRow(page, "build-runner");
    await runner.locator("button[aria-expanded]").click();
    await expect(runner.locator("pre")).toContainText("[dry-run] would run on ci@ci-runner-3.lan: df -h");

    // Every row carries its exit code badge.
    await expect(dialog(page).locator(".table-wrap").getByText("exit 0")).toHaveCount(HOST_COUNT);
  });

  test("host selection helpers and the subset payload", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);
    const modal = dialog(page);

    // "None" clears the preselection, then a single host is checked by name.
    await modal.getByRole("button", { name: "None", exact: true }).click();
    await expect(modal.getByText(`0 of ${HOST_COUNT} selected`)).toBeVisible();
    await modal.locator("aside").getByRole("checkbox", { name: /build-runner/ }).check();
    await expect(modal.getByText(`1 of ${HOST_COUNT} selected`)).toBeVisible();

    // Filtering narrows the visible picker list (name, host, user or tag match).
    await modal.getByLabel("Filter hosts").fill("api");
    await expect(modal.locator("aside").getByRole("checkbox")).toHaveCount(2);
    await modal.getByLabel("Filter hosts").fill("no-such-host-zzz");
    await expect(modal.getByText("No hosts match the filter.")).toBeVisible();
    await modal.getByLabel("Filter hosts").fill("");

    // Tag chips filter by tag; "Prod"/"Non-prod" swap the whole selection.
    await modal.getByRole("button", { name: "ci", exact: true }).click();
    await expect(modal.locator("aside").getByRole("checkbox")).toHaveCount(1);
    await modal.getByRole("button", { name: "ci", exact: true }).click();
    await modal.getByRole("button", { name: "Prod", exact: true }).click();
    await expect(modal.getByText(`4 of ${HOST_COUNT} selected`)).toBeVisible();
    await modal.getByRole("button", { name: "Non-prod", exact: true }).click();
    await expect(modal.getByText(`4 of ${HOST_COUNT} selected`)).toBeVisible();

    // Back to just the CI runner, then preview only that host.
    await modal.getByRole("button", { name: "None", exact: true }).click();
    await modal.locator("aside").getByRole("checkbox", { name: /build-runner/ }).check();
    await app.clearIpc();

    await page.getByRole("button", { name: "Preview on 1 host" }).click();
    await expect
      .poll(async () => app.ipcCalls("run_batch_command"))
      .toEqual([{ connectionIds: ["id-4"], command: "uptime", dryRun: true, timeoutSecs: 10 }]);

    const runner = resultRow(page, "build-runner");
    await expect(runner.locator("button[aria-expanded]")).toHaveAttribute("aria-expanded", "true");
    await expect(runner.locator("pre")).toContainText("ci@ci-runner-3.lan: uptime");
    await expect(page.getByText("1 passed")).toBeVisible();
  });

  test("a live run renders stdout per host and surfaces the failing host", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);

    // Turning dry run off with production hosts selected warns before running.
    await dialog(page).getByRole("checkbox", { name: "Dry run" }).uncheck();
    await expect(page.getByText("Production hosts selected.")).toBeVisible();

    await app.clearIpc();
    await page.getByRole("button", { name: `Run on ${HOST_COUNT} hosts` }).click();
    await app.waitForIpc("run_batch_command", 1);

    expect(await app.ipcCalls("run_batch_command")).toEqual([
      { connectionIds: ALL_IDS, command: "uptime", dryRun: false, timeoutSecs: 10 },
    ]);

    await expect(dialog(page).locator(".table-wrap > div")).toHaveCount(HOST_COUNT);
    await expect(page.getByText("7 passed")).toBeVisible();
    await expect(page.getByText("1 failed")).toBeVisible();

    // The failing host is expanded automatically and shows its stderr.
    const runner = resultRow(page, "build-runner");
    await expect(runner.locator(".badge")).toHaveText("exit 255");
    await expect(runner.locator("button[aria-expanded]")).toHaveAttribute("aria-expanded", "true");
    await expect(runner.locator("pre")).toContainText(
      "ssh: connect to host ci-runner-3.lan port 22: Connection timed out",
    );

    // Successful hosts are collapsed until clicked; their stdout is the uptime text.
    const api = resultRow(page, "prod-api-01");
    await expect(api.locator(".badge")).toHaveText("exit 0");
    await expect(api.locator("button[aria-expanded]")).toHaveAttribute("aria-expanded", "false");
    await api.locator("button[aria-expanded]").click();
    await expect(api.locator("pre")).toContainText("up 3 days");
  });

  test("runs are recorded in History and can be inspected", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);

    await page.getByRole("button", { name: `Preview on ${HOST_COUNT} hosts` }).click();
    await app.waitForIpc("run_batch_command", 1);

    await dialog(page).getByRole("button", { name: /^History/ }).click();
    const row = dialog(page).locator("tbody tr").filter({ hasText: "uptime" });
    await expect(row).toHaveCount(1);
    await expect(row).toContainText("Dry run");
    await expect(row).toContainText(`${HOST_COUNT}/${HOST_COUNT} passed`);

    // Inspect restores the recorded results on the Run tab.
    await row.getByRole("button", { name: "Inspect" }).click();
    await expect(page.getByText(`${HOST_COUNT} passed`)).toBeVisible();
    await expect(dialog(page).locator(".table-wrap > div")).toHaveCount(HOST_COUNT);
  });

  test.describe("environment warnings", () => {
    test.use({ mockOptions: { batchEnvWarn: true } });

    test("missing SSH_AUTH_SOCK warns until dismissed", async ({ app, page }) => {
      await app.gotoApp();
      await openFromHosts(page);

      const modal = dialog(page);
      const alert = modal.locator(".alert-warning[role], .alert-warning").first();
      await expect(alert).toBeVisible();
      await expect(alert).toContainText("SSH environment:");
      await expect(alert).toContainText(
        "SSH_AUTH_SOCK is not set; key-based hosts may prompt for a passphrase.",
      );

      await modal.getByRole("button", { name: "Dismiss warning" }).click();
      await expect(modal.locator(".alert-warning")).toHaveCount(0);
    });
  });
});
