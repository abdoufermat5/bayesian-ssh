import { test, expect } from "./fixtures/app";

/**
 * First-run onboarding stepper (OnboardingModal.svelte).
 *
 * With `needs_onboarding` true the app renders the full-screen stepper instead
 * of the shell. Driving every step to completion must send the exact
 * `complete_onboarding` payload (built from the component's real fields) and
 * then swap the stepper for `.app-shell`.
 */

test.use({ mockOptions: { onboarding: true } });

const SETUP = { role: "main" as const, name: "First-run setup" };

test("stepper drives to completion and submits the exact onboarding payload", async ({
  app,
  page,
}) => {
  await app.gotoApp();

  const setup = page.getByRole(SETUP.role, { name: SETUP.name });
  await expect(page.locator(".app-shell")).toHaveCount(0);

  // Step 1 — Welcome.
  await expect(page.getByRole("heading", { name: "Welcome to Bayesian SSH" })).toBeVisible();
  await page.getByRole("button", { name: "Get started" }).click();

  // Step 2 — Profile and defaults.
  await expect(page.getByRole("heading", { name: "Profile and defaults" })).toBeVisible();
  await page.locator("#ob-profile").fill("work");
  await page.locator("#ob-user").fill("deploy");
  await page.locator("#ob-port").fill("2222");
  await page.locator("#ob-agent").check();
  await page.getByRole("button", { name: "Continue" }).click();

  // Step 3 — OpenSSH import (host import stays on by default).
  await expect(page.getByRole("heading", { name: "Import from OpenSSH" })).toBeVisible();
  await expect(page.locator("#ob-import")).toBeChecked();
  await page.locator("#ob-ssh-config").fill("~/.ssh/config");
  await page.getByRole("button", { name: "Continue" }).click();

  // Step 4 — Appearance.
  await expect(page.getByRole("heading", { name: "Appearance" })).toBeVisible();
  await page.getByRole("radio", { name: "OLED" }).click();
  await page.locator("#ob-fuzzy").check();
  await page.getByRole("button", { name: "Continue" }).click();

  // Step 5 — Done. Submit the form via the primary action.
  await expect(page.getByRole("heading", { name: "You're all set" })).toBeVisible();
  await page.getByRole("button", { name: "Open Bayesian SSH" }).click();

  await app.waitForIpc("complete_onboarding", 1);
  const calls = (await app.ipcCalls("complete_onboarding")) as Array<{ payload: unknown }>;
  expect(calls).toHaveLength(1);
  expect(calls[0]).toEqual({
    payload: {
      profile_name: "work",
      create_profile: true,
      default_user: "deploy",
      default_port: 2222,
      ssh_config_path: "~/.ssh/config",
      theme: "oled",
      auto_start_agent: true,
      import_ssh_config: true,
      fuzzy_search: true,
    },
  });

  // Completion reveals the main shell and unmounts the stepper.
  await expect(page.locator(".app-shell")).toBeVisible();
  await expect(setup).toHaveCount(0);
});
