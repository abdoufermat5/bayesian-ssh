import { test, expect, type AppFixture, type Page } from "./fixtures/app";

/**
 * Settings view e2e coverage.
 *
 * Playwright's `page` object is addressed directly for anything the
 * `AppFixture` doesn't wrap; the fixture owns navigation + IPC access.
 */

const SECTIONS = 'nav[aria-label="Settings sections"]';

async function openSettings(app: AppFixture, page: Page) {
  await app.gotoApp();
  await app.nav("Settings");
  await expect(page.locator(".settings-shell")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Profiles & workspace", level: 2 })).toBeVisible();
}

async function openSection(page: Page, label: string) {
  const button = page.locator(SECTIONS).getByRole("button", { name: label, exact: true });
  await button.click();
  await expect(button).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("heading", { name: label, level: 2 })).toBeVisible();
}

test.describe("settings", () => {
  test("renders every section and navigates the category rail", async ({ app, page }) => {
    await openSettings(app, page);

    const sections = [
      { label: "Profiles & workspace", marker: page.getByText("Active profile") },
      { label: "SSH agent", marker: page.getByRole("checkbox", { name: /Start agent on launch/ }) },
      { label: "Kerberos", marker: page.getByRole("checkbox", { name: /Monitor ticket expiry/ }) },
      { label: "Terminal", marker: page.getByText("Font size", { exact: true }) },
      { label: "History", marker: page.getByRole("checkbox", { name: /Record session history/ }) },
      { label: "Appearance", marker: page.getByRole("radiogroup", { name: "Theme" }) },
      { label: "Features", marker: page.getByRole("checkbox", { name: /SFTP file browser/ }) },
    ];

    // The rail exposes exactly the seven documented categories.
    await expect(page.locator(SECTIONS).getByRole("button")).toHaveCount(sections.length);

    for (const { label, marker } of sections) {
      await openSection(page, label);
      await expect(marker).toBeVisible();
    }

    // Workspace section content round-trips when we come back to it.
    await openSection(page, "Profiles & workspace");
    await expect(page.getByText("Config root")).toBeVisible();
    await expect(page.getByText("/home/me/.config/bayesian-ssh", { exact: true })).toBeVisible();
  });

  test("toggling switches saves desktop settings with the changed field", async ({ app, page }) => {
    await openSettings(app, page);

    // Kerberos: monitoring starts enabled, flip it off.
    await openSection(page, "Kerberos");
    const monitor = page.getByRole("checkbox", { name: /Monitor ticket expiry/ });
    await expect(monitor).toBeChecked();
    await app.clearIpc();
    await monitor.uncheck();

    await app.waitForIpc("save_desktop_settings");
    expect((await app.ipcCalls("save_desktop_settings")).at(-1)).toMatchObject({
      settings: { monitor_kerberos: false },
    });
    // The warning threshold becomes unavailable while monitoring is off.
    await expect(page.locator("#settings-kerberos-warn")).toBeDisabled();

    // Features: disable the SFTP browser (enabled by default).
    await openSection(page, "Features");
    const sftp = page.getByRole("checkbox", { name: /SFTP file browser/ });
    await expect(sftp).toBeChecked();
    await app.clearIpc();
    await sftp.uncheck();

    await app.waitForIpc("save_desktop_settings");
    expect((await app.ipcCalls("save_desktop_settings")).at(-1)).toMatchObject({
      settings: { enable_sftp: false },
    });

    // Re-enable so the SFTP nav entry stays available.
    await sftp.check();
    await app.waitForIpc("save_desktop_settings", 2);
    expect((await app.ipcCalls("save_desktop_settings")).at(-1)).toMatchObject({
      settings: { enable_sftp: true },
    });
  });

  test("workspace fields save the workspace config", async ({ app, page }) => {
    await openSettings(app, page);

    // The SSH config path commits through the workspace save path.
    const sshConfig = page.locator("#settings-ssh-config");
    await expect(sshConfig).toHaveValue("/home/me/.ssh/config");
    await app.clearIpc();
    await sshConfig.fill("/tmp/custom_ssh_config");
    await sshConfig.blur();

    await app.waitForIpc("save_workspace_config");
    expect((await app.ipcCalls("save_workspace_config")).at(-1)).toMatchObject({
      update: { ssh_config_path: "/tmp/custom_ssh_config" },
    });

    // Connection defaults re-derive workspace fields and persist both blobs.
    await openSection(page, "SSH agent");
    await app.clearIpc();
    const defaultUser = page.locator("#settings-default-user");
    await expect(defaultUser).toHaveValue("root");
    await defaultUser.fill("deploy");
    await defaultUser.blur();

    await app.waitForIpc("save_workspace_config");
    expect((await app.ipcCalls("save_workspace_config")).at(-1)).toMatchObject({
      update: { default_user: "deploy" },
    });

    await app.waitForIpc("save_desktop_settings");
    expect((await app.ipcCalls("save_desktop_settings")).at(-1)).toMatchObject({
      settings: { default_user: "deploy" },
    });
  });

  test("appearance theme switches the html class and persists across reload", async ({ app, page }) => {
    await openSettings(app, page);
    await openSection(page, "Appearance");

    const group = page.getByRole("radiogroup", { name: "Theme" });
    await expect(group.getByRole("radio")).toHaveCount(4);
    // Default theme is Graphite/zinc.
    await expect(page.getByRole("radio", { name: "Graphite" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await expect(page.locator("html")).toHaveClass(/theme-zinc/);
    expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe("zinc");

    const themeCases = [
      { name: "Midnight", id: "cyberpunk" },
      { name: "OLED black", id: "oled" },
      { name: "Slate", id: "slate" },
      { name: "Graphite", id: "zinc" },
    ];

    let saves = 0;
    for (const { name, id } of themeCases) {
      await page.getByRole("radio", { name }).click();
      await expect(page.getByRole("radio", { name })).toHaveAttribute("aria-checked", "true");

      const classes = await page.evaluate(() => [...document.documentElement.classList]);
      expect(classes).toContain(`theme-${id}`);
      expect(classes.filter((c) => c.startsWith("theme-"))).toEqual([`theme-${id}`]);
      expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe(id);

      // Each switch persists through save_desktop_settings.
      saves += 1;
      await app.waitForIpc("save_desktop_settings", saves);
      expect((await app.ipcCalls("save_desktop_settings")).at(-1)).toMatchObject({
        settings: { theme: id },
      });
    }

    // Land on Midnight and verify the persisted theme survives a reload.
    await page.getByRole("radio", { name: "Midnight" }).click();
    await expect(page.locator("html")).toHaveClass(/theme-cyberpunk/);
    await app.waitForIpc("save_desktop_settings", ++saves);

    // Mark the current JS realm so the reload below is provably a fresh
    // document (guards the persistence assertion against a stale DOM).
    await page.evaluate(() => {
      Object.defineProperty(window, "__realm", { value: "before-reload", configurable: true });
    });
    await page.reload();
    await expect(page.locator(".app-shell")).toBeVisible();
    expect(await page.evaluate(() => "__realm" in window)).toBe(false);

    const classes = await page.evaluate(() => [...document.documentElement.classList]);
    expect(classes).toContain("theme-cyberpunk");
    expect(classes.filter((c) => c.startsWith("theme-"))).toEqual(["theme-cyberpunk"]);
    expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe("cyberpunk");

    // The UI reflects the persisted selection too.
    await app.nav("Settings");
    await openSection(page, "Appearance");
    await expect(page.getByRole("radio", { name: "Midnight" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  test("browse picks an ssh config file and import pulls hosts", async ({ app, page }) => {
    await openSettings(app, page);

    const sshConfig = page.locator("#settings-ssh-config");

    // Point the field somewhere else first so the picked path is observable.
    await sshConfig.fill("/tmp/nowhere");
    await sshConfig.blur();
    await app.waitForIpc("save_workspace_config");

    await app.clearIpc();
    await page.getByRole("button", { name: "Browse" }).click();
    await app.waitForIpc("pick_ssh_config_file");
    await expect(sshConfig).toHaveValue("/home/me/.ssh/config");
    // Browse also persists the workspace.
    await app.waitForIpc("save_workspace_config");

    await app.clearIpc();
    await page.getByRole("button", { name: "Import hosts" }).click();
    await app.waitForIpc("import_ssh_config");
    expect((await app.ipcCalls("import_ssh_config")).at(-1)).toMatchObject({
      file: "/home/me/.ssh/config",
    });
    await expect(page.getByText("Imported 2 hosts from OpenSSH config")).toBeVisible();
  });
});
