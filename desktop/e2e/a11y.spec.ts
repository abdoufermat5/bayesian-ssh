import AxeBuilder from "@axe-core/playwright";
import { test, expect, type Page } from "./fixtures/app";

type Problem = { selector: string; html: string };
type Violation = { id: string; help: string; nodes: unknown[] };

async function unnamedIconButtons(page: Page): Promise<Problem[]> {
  return page.evaluate(() => {
    const problems: Array<{ selector: string; html: string }> = [];
    const buttons = document.querySelectorAll(
      ".sidebar button, .titlebar button, .workspace button, [role='dialog'] button",
    );
    for (const el of Array.from(buttons)) {
      const button = el as HTMLButtonElement;
      if (button.offsetParent === null && button.getClientRects().length === 0) continue;
      const text = (button.textContent || "").trim();
      if (text) continue;
      const label =
        button.getAttribute("aria-label") ||
        button.getAttribute("aria-labelledby") ||
        button.getAttribute("title");
      if (!label) {
        const path = button.parentElement?.className?.toString().split(" ")[0] || "button";
        problems.push({ selector: `.${path}`, html: button.outerHTML.slice(0, 140) });
      }
    }
    return problems;
  });
}

async function violations(page: Page): Promise<Violation[]> {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
    .analyze();
  return results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
}

function describe(label: string, list: Violation[]): string[] {
  return list.flatMap((v) =>
    v.nodes.map((n) => `${label}: ${v.id} — ${n.target.join(" ")} — ${n.failureSummary ?? v.help}`),
  );
}

/** Every serious/critical WCAG 2.1 AA violation (contrast included) fails. */
async function axeReport(page: Page, label: string): Promise<string[]> {
  return describe(label, await violations(page));
}

test.describe("accessibility smoke", () => {
  test("icon-only buttons have accessible names across the shell and views", async ({ app, page }) => {
    await app.gotoApp();
    for (const view of ["Hosts", "Terminals", "Files", "Tunnels", "Keys", "Audit", "History", "Settings"]) {
      await app.nav(view);
      await expect(page.locator(".workspace h1")).toBeVisible();
      expect(await unnamedIconButtons(page), `unnamed buttons in ${view}`).toEqual([]);
    }
  });

  test("modals have no unnamed icon-only buttons", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toBeVisible();
    expect(await unnamedIconButtons(page), "unnamed buttons in host modal").toEqual([]);
    await page.keyboard.press("Escape");

    await page.getByRole("button", { name: "Keyboard shortcuts" }).click();
    await expect(page.getByRole("dialog").filter({ hasText: "Keyboard shortcuts" })).toBeVisible();
    expect(await unnamedIconButtons(page), "unnamed buttons in shortcuts modal").toEqual([]);
  });

  test("main views have no serious axe violations", async ({ app, page }) => {
    await app.gotoApp();
    const report: string[] = [];
    for (const view of ["Hosts", "Keys", "History", "Settings"]) {
      await app.nav(view);
      await expect(page.locator(".workspace h1")).toBeVisible();
      report.push(...(await axeReport(page, view)));
    }
    expect(report, report.join("\n")).toEqual([]);
  });

  test("the command palette and host modal have no serious axe violations", async ({ app, page }) => {
    await app.gotoApp();
    const report: string[] = [];

    await app.openPalette();
    report.push(...(await axeReport(page, "palette")));
    await page.keyboard.press("Escape");

    await page.keyboard.press("n");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toBeVisible();
    report.push(...(await axeReport(page, "host modal")));
    expect(report, report.join("\n")).toEqual([]);
  });
});
