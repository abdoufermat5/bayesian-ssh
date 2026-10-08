import { test, expect, type AppFixture, type Page } from "./fixtures/app";

/**
 * SFTP Files view end-to-end coverage.
 *
 * Connect flow + remote listing, breadcrumb/path navigation, the hidden-files
 * toggle and its `sftp_show_hidden_files` setting, the empty-directory state,
 * the read error alert, and a connect failure surfaced by `list_remote_directory`.
 *
 * The backend mock returns 13 entries for any readable path (5 dirs incl. `app`,
 * 4 dotfiles incl. `.ssh`, plus `README.md`), returns `[]` for `/srv`, and throws
 * for `/root` (Permission denied) and for the `edge-proxy-eu` host (timeout).
 */

async function openFiles(app: AppFixture, page: Page) {
  await app.gotoApp();
  await app.nav("Files");
  await expect(page.getByRole("heading", { level: 1, name: "Files" })).toBeVisible();
}

/** Wait for the header Connect button to be usable, press it, await the listing call. */
async function clickConnect(app: AppFixture, page: Page) {
  const connect = page.getByRole("button", { name: "Connect", exact: true });
  await expect(connect).toBeEnabled();
  await connect.click();
  await app.waitForIpc("list_remote_directory");
}

/** Wait for hosts to arrive, then connect to the default host. */
async function connectDefaultHost(app: AppFixture, page: Page) {
  await expect(page.getByRole("combobox")).toContainText("prod-api-01");
  await clickConnect(app, page);
}

async function selectHost(page: Page, optionName: string) {
  // The picker is disabled until hosts load; wait for the default host first.
  await expect(page.getByRole("combobox")).toContainText("prod-api-01");
  await page.getByRole("combobox").click();
  await page.getByRole("option", { name: optionName }).click();
  await expect(page.getByRole("combobox")).toContainText(optionName.split(" ")[0]);
}

/** Type a path into the "Remote path" inline editor and submit it. */
async function goToPath(page: Page, path: string) {
  await page.getByRole("button", { name: "Edit path" }).click();
  const input = page.getByRole("textbox", { name: "Remote path" });
  await expect(input).toBeVisible();
  await input.fill(path);
  await input.press("Enter");
}

async function lastRemotePath(app: AppFixture) {
  const calls = (await app.ipcCalls("list_remote_directory")) as Array<{ remotePath?: string }>;
  return calls[calls.length - 1]?.remotePath;
}

test("connects to the selected host and renders the remote listing", async ({ app, page }) => {
  await openFiles(app, page);

  // Not connected yet: the empty state invites a connection to the default host.
  await expect(page.getByText("Not connected")).toBeVisible();
  await expect(page.getByText("Browse remote files")).toBeVisible();

  await connectDefaultHost(app, page);

  // The listing came back for the picked host at the current path.
  await expect.poll(async () => {
    const calls = (await app.ipcCalls("list_remote_directory")) as Array<{
      connectionName?: string;
      remotePath?: string;
    }>;
    return calls[0];
  }).toEqual({ connectionName: "prod-api-01", remotePath: "/" });

  // 13 entries: 5 folders + 8 files.
  await expect(page.locator("tbody tr")).toHaveCount(13);
  await expect(page.getByText("5 folders · 8 files", { exact: false })).toBeVisible();

  // Dotfiles are visible by default (`sftp_show_hidden_files` defaults true).
  await expect(page.getByRole("button", { name: ".ssh", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "app", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "README.md", exact: true })).toBeVisible();

  // The connected host is shown in the header.
  await expect(page.getByText("deploy@10.0.4.12", { exact: true })).toBeVisible();
});

test("navigates into a folder via breadcrumb and back to root", async ({ app, page }) => {
  await openFiles(app, page);
  await connectDefaultHost(app, page);

  await page.getByRole("button", { name: "app", exact: true }).click();

  // The folder click re-lists the joined path.
  await expect.poll(() => lastRemotePath(app)).toBe("/app");
  const path = page.getByRole("navigation", { name: "Path" });
  await expect(path.getByRole("button", { name: "app", exact: true })).toBeVisible();

  // ...and the root breadcrumb takes us back to "/".
  await path.getByRole("button", { name: "/", exact: true }).click();
  await expect.poll(() => lastRemotePath(app)).toBe("/");
  await expect(path.getByRole("button", { name: "app", exact: true })).toHaveCount(0);
});

test("hidden-files toggle hides and shows dotfiles", async ({ app, page }) => {
  await openFiles(app, page);
  await connectDefaultHost(app, page);

  const toggle = page.getByRole("button", { name: /Hidden files/ });
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: ".ssh", exact: true })).toBeVisible();

  // Hide dotfiles: the 4 hidden entries disappear and the count badge appears.
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("button", { name: ".ssh", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: ".env", exact: true })).toHaveCount(0);
  await expect(toggle).toContainText("4");
  // Two dotfile dirs and two dotfile files drop out: 3 folders + 6 files remain.
  await expect(page.getByText("3 folders · 6 files", { exact: false })).toBeVisible();
  await expect(page.locator("tbody tr")).toHaveCount(9);

  // Show them again.
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: ".ssh", exact: true })).toBeVisible();
});

test("honours the sftp_show_hidden_files setting", async ({ app, page }) => {
  await app.gotoApp();

  // Turn the setting off in Settings → Terminal.
  await app.nav("Settings");
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("button", { name: "Terminal", exact: true })
    .click();
  const setting = page.getByRole("checkbox", { name: /Show hidden files in SFTP/ });
  await expect(setting).toBeChecked();
  await setting.uncheck();
  await expect(setting).not.toBeChecked();

  // A fresh Files view starts from the persisted setting: dotfiles hidden.
  await app.nav("Files");
  await expect(page.getByRole("heading", { level: 1, name: "Files" })).toBeVisible();
  await connectDefaultHost(app, page);

  const toggle = page.getByRole("button", { name: /Hidden files/ });
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await expect(toggle).toContainText("4");
  await expect(page.getByRole("button", { name: ".ssh", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "app", exact: true })).toBeVisible();
});

test("shows the empty state for an empty remote folder", async ({ app, page }) => {
  await openFiles(app, page);
  await connectDefaultHost(app, page);

  await goToPath(page, "/srv");
  await expect.poll(() => lastRemotePath(app)).toBe("/srv");

  await expect(page.getByText("This folder is empty")).toBeVisible();
  await expect(page.getByText("/srv has no files or folders.", { exact: true })).toBeVisible();
  await expect(page.getByText("0 folders · 0 files", { exact: false })).toBeVisible();
  await expect(page.locator("tbody tr")).toHaveCount(0);
});

test("shows an error alert when a directory cannot be read", async ({ app, page }) => {
  app.ignoreErrors(/Permission denied/);

  await openFiles(app, page);
  await connectDefaultHost(app, page);

  await goToPath(page, "/root");
  await expect.poll(() => lastRemotePath(app)).toBe("/root");

  const alert = page.getByText("Couldn't read this directory", { exact: true });
  await expect(alert).toBeVisible();
  await expect(
    page.getByText("ls: cannot open directory '/root': Permission denied", { exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();

  // The failed listing does not replace the previous one.
  await expect(page.locator("tbody tr")).toHaveCount(0);
});

test("surfaces a connection failure from the listing call", async ({ app, page }) => {
  app.ignoreErrors(/Connection timed out/);

  await openFiles(app, page);
  await selectHost(page, "edge-proxy-eu (root@eu1.edge.example.net)");

  await clickConnect(app, page);

  await expect(page.getByText("Couldn't connect to edge-proxy-eu", { exact: true })).toBeVisible();
  await expect(
    page.getByText("ssh: connect to host eu1.edge.example.net port 22: Connection timed out", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();

  // Still disconnected: the header keeps the "Not connected" label.
  await expect(page.getByText("Not connected")).toBeVisible();
});
