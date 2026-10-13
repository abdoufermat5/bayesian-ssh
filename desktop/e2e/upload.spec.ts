import { test, expect, type Page } from "./fixtures/app";

/**
 * Upload modal (`UploadModal.svelte`) — copying files to many hosts at once.
 *
 * The component invokes `start_upload` and then consumes `upload-event` /
 * `upload-finished` Tauri events; there is no Rust in the browser, so the mock
 * streams those events itself (`simulateUpload` in `fixtures/tauri-mock.js`).
 * Deterministic outcomes: `build-runner` always fails, `homelab-nas` reports
 * `viaScp` (no progress events), every other host succeeds.
 */

const HOST_COUNT = 8;
const FILE = "/home/me/notes.md";
const STAGING = "id-3";
const BUILD = "id-4";

const dialog = (page: Page) => page.getByRole("dialog", { name: "Upload files to hosts" });
/** Browser window augmented with the mock's helpers (`fixtures/tauri-mock.js`). */
type MockWindow = Window & {
  __MOCK_EMIT(event: string, payload: unknown): void;
  __UPLOAD_DELAY?: number;
};
/** A per-host result row inside the results table. */
const row = (page: Page, host: string) =>
  dialog(page).locator(".table-wrap > div").filter({ hasText: host });

async function openFromHosts(page: Page) {
  await page.locator(".view-header").getByRole("button", { name: "Upload files" }).click();
  await expect(dialog(page)).toBeVisible();
}

async function addFile(page: Page) {
  await dialog(page).getByRole("button", { name: "Add files" }).click();
  await expect(dialog(page).getByText(FILE)).toBeVisible();
}

async function pickHost(page: Page, name: string) {
  await dialog(page).locator("aside label").filter({ hasText: name }).click();
}

async function startWith(page: Page, hosts: string[]) {
  await openFromHosts(page);
  await addFile(page);
  for (const host of hosts) await pickHost(page, host);
  await dialog(page).getByRole("button", { name: `Upload to ${hosts.length} host${hosts.length === 1 ? "" : "s"}` }).click();
}

test.describe("upload modal", () => {
  test("opens from the Hosts view with no host preselected", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);

    const modal = dialog(page);
    await expect(
      modal.getByText("Copy files or folders to several servers at once, in parallel."),
    ).toBeVisible();
    await expect(modal.getByText(`0 of ${HOST_COUNT} selected`)).toBeVisible();
    await expect(modal.locator("aside").getByRole("checkbox")).toHaveCount(HOST_COUNT);
    await expect(page.locator("#upload-remote-dir")).toHaveValue("/");
    await expect(modal.getByText("No upload yet")).toBeVisible();
    await expect(modal.getByRole("button", { name: `Upload to 0 hosts` })).toBeDisabled();
  });

  test("opens from the command palette with no host preselected", async ({ app, page }) => {
    await app.gotoApp();
    await app.openPalette();
    const palette = page.getByRole("dialog", { name: "Command palette" });
    await palette.getByRole("combobox", { name: "Search commands" }).fill("upload");
    await palette.getByRole("option", { name: /Upload files/ }).click();

    await expect(dialog(page)).toBeVisible();
    await expect(dialog(page).getByText(`0 of ${HOST_COUNT} selected`)).toBeVisible();
  });

  test("opens from the Files view with host and directory preselected", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Files");
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByText("docker-compose.yml")).toBeVisible();

    await page.getByRole("button", { name: "Upload", exact: true }).click();
    await expect(dialog(page)).toBeVisible();
    await expect(dialog(page).getByText(`1 of ${HOST_COUNT} selected`)).toBeVisible();
    await expect(page.locator("#upload-remote-dir")).toHaveValue("/");
  });

  test("accepts files dropped onto the modal", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);

    // The webview drag-drop listener registers asynchronously; retry the drop
    // until it lands (duplicates are de-duplicated by the modal).
    await expect
      .poll(
        async () => {
          await page.evaluate(() => {
            const mockWindow = window as unknown as MockWindow; // injected by fixtures/tauri-mock.js
            mockWindow.__MOCK_EMIT("tauri://drag-drop", {
              paths: ["/home/me/dropped.txt"],
              position: { x: 10, y: 10 },
            });
          });
          return dialog(page).getByText("/home/me/dropped.txt").isVisible();
        },
        { timeout: 5000 },
      )
      .toBe(true);
  });

  test("uploads to the selected hosts and streams per-host progress", async ({ app, page }) => {
    await app.gotoApp();
    await openFromHosts(page);
    await addFile(page);
    await page.locator("#upload-remote-dir").fill("/srv/app");
    await pickHost(page, "staging-web");
    await pickHost(page, "build-runner");
    await expect(dialog(page).getByText("2 of 8 selected")).toBeVisible();

    await dialog(page).getByRole("button", { name: "Upload to 2 hosts" }).click();

    await expect(row(page, "staging-web").getByText("Done")).toBeVisible();
    await expect(row(page, "build-runner").getByText("Failed")).toBeVisible();
    await expect(row(page, "build-runner").getByText(/Connection timed out/)).toBeVisible();
    await expect(dialog(page).getByText("1 of 2 hosts done")).toBeVisible();
    await expect(dialog(page).getByText("1 failed")).toBeVisible();

    expect(await app.ipcCalls("start_upload")).toEqual([
      {
        connectionIds: [STAGING, BUILD],
        localPaths: [FILE],
        remoteDir: "/srv/app",
        parallel: null,
      },
    ]);
  });

  test("counts hosts, not file × host transfers, when uploading several sources", async ({
    app,
    page,
  }) => {
    await app.gotoApp();
    await openFromHosts(page);
    await addFile(page);
    await dialog(page).getByRole("button", { name: "Add folder" }).click();
    await expect(dialog(page).getByText("/home/me/project")).toBeVisible();
    await pickHost(page, "staging-web");
    await pickHost(page, "homelab-nas");
    await dialog(page).getByRole("button", { name: "Upload to 2 hosts" }).click();

    await expect(dialog(page).getByText("2 of 2 hosts done")).toBeVisible();
    await expect(page.getByText("Upload finished: 2 hosts updated")).toBeVisible();
  });

  test("tags hosts that fall back to SCP", async ({ app, page }) => {
    await app.gotoApp();
    await startWith(page, ["homelab-nas"]);

    await expect(row(page, "homelab-nas").getByText("via SCP")).toBeVisible();
    await expect(row(page, "homelab-nas").getByText("Done")).toBeVisible();
    await expect(dialog(page).getByText("1 of 1 hosts done")).toBeVisible();
  });

  test("retry re-runs only the failed host", async ({ app, page }) => {
    await app.gotoApp();
    await startWith(page, ["staging-web", "build-runner"]);
    await expect(row(page, "build-runner").getByText("Failed")).toBeVisible();

    await app.clearIpc();
    await dialog(page).getByRole("button", { name: "Retry failed" }).click();
    await app.waitForIpc("start_upload");
    await expect(row(page, "build-runner").getByText("Failed")).toBeVisible();

    const calls = await app.ipcCalls("start_upload");
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({
      connectionIds: [BUILD],
      localPaths: [FILE],
      remoteDir: "/",
    });
    // The host that already succeeded is not uploaded again.
    await expect(row(page, "staging-web").getByText("Done")).toBeVisible();
  });

  test("cancelling aborts the job and marks pending hosts", async ({ app, page }) => {
    await page.addInitScript(() => {
      const mockWindow = window as unknown as MockWindow; // injected by fixtures/tauri-mock.js
      mockWindow.__UPLOAD_DELAY = 400;
    });
    await app.gotoApp();
    await startWith(page, ["staging-web"]);

    await expect(row(page, "staging-web").getByText("Uploading")).toBeVisible();
    await dialog(page).getByRole("button", { name: "Cancel" }).click();

    await expect(row(page, "staging-web").getByText("Cancelled")).toBeVisible();
    await expect(dialog(page).getByText("0 of 1 hosts done")).toBeVisible();
    expect(await app.ipcCalls("cancel_upload")).toHaveLength(1);
  });
});
