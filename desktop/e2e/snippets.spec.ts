import { test, expect, type Page } from "./fixtures/app";

/**
 * Snippets modal (`SnippetsModal.svelte`).
 *
 * Snippets live in `localStorage["bayesian-ssh-snippets"]` and are written on
 * every edit. Running one sends it to the active SSH terminal in `+page.svelte`
 * (`term.paste(cmd)` + `term.input("\r")` => two `write_pty` calls), or falls
 * back to the clipboard with a toast when no terminal tab is open.
 */

const STORAGE_KEY = "bayesian-ssh-snippets";
const TAIL_CMD = "tail -n 100 -f /var/log/syslog | grep -i error";

const DEFAULT_TITLES = [
  "Tail system error logs",
  "Disk usage and largest directories",
  "Follow Docker container logs",
  "Nginx status and errors",
  "Listening ports",
  "Top processes by CPU",
];

const dialog = (page: Page) => page.getByRole("dialog", { name: "Snippets" });

const option = (page: Page, name: string | RegExp) =>
  dialog(page).getByRole("option", { name });

const storedSnippets = (page: Page) =>
  page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "[]"), STORAGE_KEY);

test.describe("snippets modal", () => {
  test("opens from the sidebar and lists the built-in snippets", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Snippets");

    const modal = dialog(page);
    await expect(modal).toBeVisible();
    await expect(modal.getByText("Saved commands you can copy or send to the active terminal.")).toBeVisible();

    const list = modal.getByRole("listbox", { name: "Snippets" });
    await expect(list.getByRole("option")).toHaveCount(DEFAULT_TITLES.length);
    for (const title of DEFAULT_TITLES) {
      await expect(option(page, new RegExp(title))).toBeVisible();
    }

    // The first snippet is selected and loaded into the editor.
    await expect(option(page, /Tail system error logs/)).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#snippet-title")).toHaveValue("Tail system error logs");
    await expect(page.locator("#snippet-command")).toHaveValue(TAIL_CMD);

    // Category chips filter the list.
    await modal.getByRole("button", { name: "Docker", exact: true }).click();
    await expect(list.getByRole("option")).toHaveCount(1);
    await expect(option(page, /Follow Docker container logs/)).toBeVisible();
    await modal.getByRole("button", { name: "All", exact: true }).click();
    await expect(list.getByRole("option")).toHaveCount(DEFAULT_TITLES.length);
  });

  test("creates, edits and deletes snippets, persisting each change", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Snippets");
    const modal = dialog(page);

    // Create.
    await modal.getByRole("button", { name: "New snippet" }).click();
    const created = option(page, /Untitled snippet/);
    await expect(created).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#snippet-title")).toHaveValue("Untitled snippet");
    await expect(page.locator("#snippet-category")).toHaveValue("General");
    await expect(page.locator("#snippet-command")).toHaveValue("");
    // An empty command cannot be copied or run.
    await expect(modal.getByRole("button", { name: "Run in terminal" })).toBeDisabled();

    await page.locator("#snippet-title").fill("Restart nginx");
    await page.locator("#snippet-category").fill("Ops");
    await page.locator("#snippet-description").fill("Reload the web tier");
    await page.locator("#snippet-command").fill("sudo systemctl restart nginx");

    await expect
      .poll(async () => (await storedSnippets(page))[0])
      .toMatchObject({
        title: "Restart nginx",
        category: "Ops",
        description: "Reload the web tier",
        command: "sudo systemctl restart nginx",
      });
    await expect(modal.getByRole("button", { name: "Run in terminal" })).toBeEnabled();

    // Edit an existing (built-in) snippet — the change is persisted too.
    await option(page, /Listening ports/).click();
    await expect(page.locator("#snippet-command")).toHaveValue("ss -tulpn | grep LISTEN");
    await page.locator("#snippet-command").fill("ss -tulpn");
    await expect
      .poll(async () => (await storedSnippets(page)).find((s: { title: string }) => s.title === "Listening ports")?.command)
      .toBe("ss -tulpn");

    // Delete removes it from both the list and storage.
    await option(page, /Restart nginx/).click();
    await modal.getByRole("button", { name: "Delete" }).click();

    await expect(option(page, /Restart nginx/)).toHaveCount(0);
    await expect(modal.getByRole("listbox", { name: "Snippets" }).getByRole("option")).toHaveCount(
      DEFAULT_TITLES.length,
    );
    await expect
      .poll(async () => (await storedSnippets(page)).length)
      .toBe(DEFAULT_TITLES.length);
    await expect
      .poll(async () => (await storedSnippets(page)).some((s: { title: string }) => s.title === "Restart nginx"))
      .toBe(false);
    await expect(page.getByRole("status").filter({ hasText: "Snippet deleted" })).toBeVisible();
  });

  test("snippets survive a page reload", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Snippets");
    const modal = dialog(page);

    await modal.getByRole("button", { name: "New snippet" }).click();
    await page.locator("#snippet-title").fill("Disk sweep");
    await page.locator("#snippet-category").fill("Maintenance");
    await page.locator("#snippet-command").fill("du -sh /var/* | sort -rh");
    await expect
      .poll(async () => (await storedSnippets(page)).some((s: { title: string }) => s.title === "Disk sweep"))
      .toBe(true);

    await page.reload();
    await expect(page.locator(".app-shell")).toBeVisible();

    await app.nav("Snippets");
    const reloaded = dialog(page);
    await expect(reloaded).toBeVisible();
    await expect(option(page, /Disk sweep/)).toBeVisible();
    await expect(option(page, /Listening ports/)).toBeVisible();

    await option(page, /Disk sweep/).click();
    await expect(page.locator("#snippet-title")).toHaveValue("Disk sweep");
    await expect(page.locator("#snippet-command")).toHaveValue("du -sh /var/* | sort -rh");

    // Selecting a category makes the new snippet reachable through the chips.
    await reloaded.getByRole("button", { name: "Maintenance", exact: true }).click();
    await expect(reloaded.getByRole("listbox", { name: "Snippets" }).getByRole("option")).toHaveCount(1);
  });

  test.describe("running a snippet", () => {
    test.use({ permissions: ["clipboard-read", "clipboard-write"] });

    test("with no active terminal it copies the command and toasts", async ({ app, page }) => {
      await app.gotoApp();
      await app.nav("Snippets");

      await expect(page.locator("#snippet-command")).toHaveValue(TAIL_CMD);
      await app.clearIpc();
      await dialog(page).getByRole("button", { name: "Run in terminal" }).click();

      // Real behaviour when `terminalState.activeTab.term` is undefined.
      await expect(
        page.getByRole("status").filter({ hasText: "No active terminal — command copied to clipboard" }),
      ).toBeVisible();
      await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(TAIL_CMD);
      await expect(dialog(page)).toHaveCount(0);
      expect(await app.ipcCalls("write_pty")).toHaveLength(0);
    });

    test("with an active terminal it confirms, then writes the command to the pty", async ({
      app,
      page,
    }) => {
      await app.gotoApp();
      await app.nav("Terminals");
      await page.getByTitle("Connect to prod-api-01").click();
      await app.waitForIpc("spawn_pty");
      const [{ sessionId }] = (await app.ipcCalls("spawn_pty")) as Array<{ sessionId: string }>;
      await expect(page.getByRole("tabpanel", { name: "prod-api-01" })).toBeVisible();

      await app.nav("Snippets");
      await expect(page.locator("#snippet-command")).toHaveValue(TAIL_CMD);
      await app.clearIpc();

      await dialog(page).getByRole("button", { name: "Run in terminal" }).click();

      // `confirm_snippet_execution` defaults to true.
      const confirm = page.getByRole("dialog", { name: "Execute command snippet" });
      await expect(confirm).toBeVisible();
      await expect(confirm).toContainText(TAIL_CMD);
      expect(await app.ipcCalls("write_pty")).toHaveLength(0);

      await confirm.getByRole("button", { name: "Execute" }).click();

      await expect.poll(async () => app.ipcCalls("write_pty")).toEqual([
        { sessionId, data: TAIL_CMD },
        { sessionId, data: "\r" },
      ]);

      // The modal closes once the snippet is dispatched.
      await expect(dialog(page)).toHaveCount(0);
    });
  });
});
