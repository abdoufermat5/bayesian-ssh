import { test as base, expect, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Playwright fixture that boots the SvelteKit SPA with the Tauri IPC mock
 * (`tauri-mock.js`) injected before any app code runs.
 *
 * Use it through `mockOptions` (per test / per describe):
 *
 * ```ts
 * import { test, expect } from "../fixtures/app";
 * test("...", async ({ app, page }) => {
 *   await app.gotoApp();
 *   await app.nav("Keys");
 * });
 *
 * test.describe("kerberos", () => {
 *   test.use({ mockOptions: { kerberos: "none" } });
 * });
 * ```
 */
export type MockOverride = (args: unknown) => unknown;

export type MockOptions = {
  /** Start in the onboarding flow instead of the main app. */
  onboarding?: boolean;
  /** Kerberos status shape the mock reports. Defaults to a valid ticket. */
  kerberos?: "valid" | "none" | "notools";
  /** Detached background sessions reported by `list_detached_sessions`. */
  detached?: Array<{ session_id: string; connection_name: string }>;
  /** Pop-out sessions reported by `list_popout_sessions`. */
  popouts?: Array<{ session_id: string; connection_name: string; window_label?: string }>;
  /** Make `get_env_status` report SSH_AUTH_SOCK warnings. */
  batchEnvWarn?: boolean;
  /** Artificial delay (ms) for `list_remote_directory`. */
  sftpDelay?: number;
  /** Artificial delay (ms) for `run_security_audit`. */
  auditDelay?: number;
  /** Artificial delay (ms) for `run_batch_command`. */
  batchDelay?: number;
  /**
   * Replace built-in IPC handlers. The function source is stringified into an
   * init script, so it MUST be self-contained (do not close over test scope;
   * use `page.evaluate` for that). Throwing surfaces the value to the UI.
   */
  overrides?: Record<string, MockOverride>;
};

/** `window` augmented with the mock's recorder helpers. */
type MockWindow = Window & {
  __IPC_CALLS(cmd: string): unknown[];
  __MOCK_RESET(): void;
};

const mockWindow = () => window as unknown as MockWindow;

type ErrorState = { errors: string[]; ignored: RegExp[] };
const errorStates = new WeakMap<Page, ErrorState>();

const here = path.dirname(fileURLToPath(import.meta.url));
const MOCK_SOURCE = fs.readFileSync(path.join(here, "tauri-mock.js"), "utf8");

function optionsScript(options: MockOptions): string {
  const globals: Record<string, unknown> = {};
  if (options.onboarding) globals.__ONBOARD = true;
  if (options.kerberos) globals.__KRB_MODE = options.kerberos;
  if (options.detached) globals.__MOCK_DETACHED = options.detached;
  if (options.popouts) globals.__MOCK_POPOUTS = options.popouts;
  if (options.batchEnvWarn) globals.__BATCH_ENV_WARN = true;
  if (options.sftpDelay) globals.__SFTP_DELAY = options.sftpDelay;
  if (options.auditDelay) globals.__AUDIT_DELAY = options.auditDelay;
  if (options.batchDelay) globals.__BATCH_DELAY = options.batchDelay;

  const lines = Object.entries(globals).map(([k, v]) => `window.${k} = ${JSON.stringify(v)};`);
  const overrides = options.overrides
    ? Object.entries(options.overrides).map(([cmd, fn]) => `${JSON.stringify(cmd)}: (${fn.toString()})`)
    : [];
  if (overrides.length) lines.push(`window.__MOCK_OVERRIDES = {${overrides.join(",")}};`);
  return lines.join("\n");
}

export type AppFixture = {
  /** Navigate to the app and wait for the shell (or onboarding) to render. */
  gotoApp(): Promise<void>;
  /** Click a sidebar nav item by its accessible name. */
  nav(label: string): Promise<void>;
  /** All argument objects recorded for one IPC command, in call order. */
  ipcCalls(cmd: string): Promise<unknown[]>;
  /** Wait until `cmd` has been recorded at least `count` times. */
  waitForIpc(cmd: string, count?: number): Promise<void>;
  /** Open the command palette via Ctrl+K. */
  openPalette(): Promise<void>;
  /** Switch views directly through the app store (fast path). */
  setActiveTab(tab: string): Promise<void>;
  /** Clear the recorded IPC log. */
  clearIpc(): Promise<void>;
  /** Assert that no page errors / console errors were collected. */
  expectNoPageErrors(): Promise<void>;
  /** Ignore errors matching a pattern for the rest of the test. */
  ignoreErrors(pattern: RegExp): void;
};

type Fixtures = {
  mockOptions: MockOptions;
  app: AppFixture;
  _errorGuard: void;
};

export const test = base.extend<Fixtures>({
  mockOptions: [{}, { option: true }],

  _errorGuard: [
    async ({ page }, use) => {
      const state: ErrorState = { errors: [], ignored: [] };
      errorStates.set(page, state);
      page.on("pageerror", (err) => {
        const text = `pageerror: ${err.message}`;
        if (!state.ignored.some((re) => re.test(text))) state.errors.push(text);
      });
      page.on("console", (msg) => {
        if (msg.type() !== "error") return;
        const text = `console.error: ${msg.text()}`;
        if (!state.ignored.some((re) => re.test(text))) state.errors.push(text);
      });
      await use();
      if (state.errors.length) {
        throw new Error(`The app produced ${state.errors.length} error(s):\n${state.errors.join("\n")}`);
      }
    },
    { auto: true },
  ],

  app: async ({ page, mockOptions }, use) => {
    const stateOf = (): ErrorState => {
      const state = errorStates.get(page);
      if (!state) throw new Error("error guard fixture was not initialized");
      return state;
    };

    const app: AppFixture = {
      async gotoApp() {
        await page.goto("/");
        if (mockOptions.onboarding) {
          await expect(page.getByRole("main", { name: "First-run setup" })).toBeVisible({ timeout: 15000 });
        } else {
          await expect(page.locator(".app-shell")).toBeVisible({ timeout: 15000 });
          await expect(page.locator(".sidebar")).toBeVisible();
        }
      },
      async nav(label) {
        await page.locator(".sidebar").getByLabel(label, { exact: true }).first().click();
      },
      async ipcCalls(cmd) {
        return page.evaluate((c) => (window as unknown as MockWindow).__IPC_CALLS(c), cmd);
      },
      async waitForIpc(cmd, count = 1) {
        await expect
          .poll(async () => (await app.ipcCalls(cmd)).length, { timeout: 10000 })
          .toBeGreaterThanOrEqual(count);
      },
      async openPalette() {
        await page.keyboard.press("Control+k");
        await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
      },
      async setActiveTab(tab) {
        // Switch through the sidebar rather than importing the store: a fresh
        // `import()` of the module URL in dev creates a second store instance
        // (the app loaded it with an HMR `?t=` query), so mutations wouldn't
        // reach the running app.
        const labels: Record<string, string> = {
          connections: "Hosts",
          terminals: "Terminals",
          sftp: "Files",
          tunnels: "Tunnels",
          keys: "Keys",
          audit: "Audit",
          history: "History",
          settings: "Settings",
        };
        const label = labels[tab];
        if (!label) throw new Error(`unknown tab: ${tab}`);
        await app.nav(label);
      },
      async clearIpc() {
        await page.evaluate(() => (window as unknown as MockWindow).__MOCK_RESET());
      },
      async expectNoPageErrors() {
        const state = stateOf();
        expect(state.errors, state.errors.join("\n")).toEqual([]);
      },
      ignoreErrors(pattern) {
        stateOf().ignored.push(pattern);
      },
    };

    // Register the option globals first, then the mock itself: init scripts run
    // in registration order, so `__MOCK_OVERRIDES` exists before `invoke` runs.
    await page.addInitScript({ content: optionsScript(mockOptions) });
    await page.addInitScript({ content: MOCK_SOURCE });

    await use(app);
  },
});

export { expect };
export type { Page };
