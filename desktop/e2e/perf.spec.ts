import { test, expect } from "./fixtures/app";

/**
 * Generates 2,000 hosts and honors the app's search/tag contract.
 * Self-contained because the fixture stringifies it into the page.
 */
const manyHosts = (args: unknown) => {
  const a = (args || {}) as { query?: string; tagFilter?: string | null };
  const all = Array.from({ length: 2000 }, (_, i) => ({
    id: "h" + i,
    name: "host-" + String(i).padStart(4, "0"),
    host: `10.0.${i >> 8}.${i % 256}`,
    user: "deploy",
    port: 22,
    tags: i % 7 === 0 ? ["prod"] : ["dev"],
    use_kerberos: false,
    created_at: "2026-01-01T00:00:00Z",
  }));
  let out = all;
  if (a.query) out = out.filter((c) => (c.name + c.host + c.user).includes(a.query!));
  if (a.tagFilter) out = out.filter((c) => c.tags.includes(a.tagFilter!));
  return out;
};

test.describe("performance smoke", () => {
  test.use({ mockOptions: { overrides: { get_connections: manyHosts } } });

  test("renders 2,000 hosts windowed and filters within budget", async ({ app, page }) => {
    test.setTimeout(90_000);

    await app.gotoApp();
    const table = page.locator("table.data-table");
    await expect(table).toHaveAttribute("aria-rowcount", "2000", { timeout: 30_000 });
    const hostRows = page.locator("tbody tr[data-host-row]");
    await expect(hostRows.first()).toBeVisible();

    // Only the rows in view (+ overscan) are mounted, not all 2,000.
    const mounted = await hostRows.count();
    expect(mounted).toBeGreaterThan(5);
    expect(mounted).toBeLessThan(120);

    // A full list <-> grid re-render stays fast because nothing off-screen mounts.
    const rerenderMs = await page.evaluate(async () => {
      const grid = document.querySelector<HTMLButtonElement>('button[aria-label="Grid layout"]')!;
      const list = document.querySelector<HTMLButtonElement>('button[aria-label="List layout"]')!;
      // Resolves after the frame that renders the click has been painted.
      const afterPaint = () => {
        const { promise, resolve } = Promise.withResolvers<void>();
        requestAnimationFrame(() => setTimeout(resolve));
        return promise;
      };
      const s = performance.now();
      grid.click();
      await afterPaint();
      list.click();
      await afterPaint();
      return performance.now() - s;
    });

    // Scrolling to the end mounts the last host.
    await page.locator(".view-body").evaluate((el) => {
      el.scrollTop = el.scrollHeight;
      el.dispatchEvent(new Event("scroll"));
    });
    await expect(page.getByText("host-1999", { exact: true })).toBeVisible();

    const filterStart = await page.evaluate(() => performance.now());
    await page.locator(".search-input").fill("host-1999");
    await expect(table).toHaveAttribute("aria-rowcount", "1");
    await expect(hostRows).toHaveCount(1);
    const filterMs = (await page.evaluate(() => performance.now())) - filterStart;

    // eslint-disable-next-line no-console
    console.log(`[perf] 2000 hosts: mounted ${mounted} rows, grid+list re-render ${Math.round(rerenderMs)}ms, filter ${Math.round(filterMs)}ms`);
    expect(rerenderMs).toBeLessThan(1500);
    expect(filterMs).toBeLessThan(1500);

    await expect.poll(async () => app.ipcCalls("get_connections")).toEqual(
      expect.arrayContaining([expect.objectContaining({ query: "host-1999" })]),
    );

    // Clearing restores the full list.
    await page.getByRole("button", { name: /clear/i }).first().click();
    await expect(table).toHaveAttribute("aria-rowcount", "2000", { timeout: 30_000 });
  });

  test("the terminals panel is display:none while another view is active", async ({ app, page }) => {
    await app.gotoApp();
    // Connect a host so the terminals panel stays mounted in the background.
    await page.keyboard.press("Enter");
    await expect(page.locator(".workspace h1")).toHaveText("Terminals");

    await page.keyboard.press("1");
    await expect(page.getByRole("heading", { level: 1, name: "Hosts" })).toBeVisible();

    const display = await page.evaluate(() => {
      const heading = Array.from(document.querySelectorAll("h1")).find((h) => h.textContent === "Terminals");
      const panel = heading?.closest(".view");
      return panel ? getComputedStyle(panel).display : null;
    });
    expect(display).toBe("none");
  });
});
