import AxeBuilder from "@axe-core/playwright";
import { test, expect, type AppFixture, type Page } from "./fixtures/app";
import type { AiApprovalRequest, AiRun, AiStoredEvent } from "../src/lib/types";

/**
 * bssh AI in the desktop app: the live view, the approval dialog, Stop and the
 * read-only policy settings. The Rust side is replaced by the IPC mock; runs
 * and requests are pushed the way `commands/ai.rs` does, through the
 * `ai://runs`, `ai://event` and `ai://request` events.
 */

const RUN_ID = "run-7f3a9c1e-0000-4000-8000-000000000001";
const NOW = Date.now();
const iso = (minutesAgo: number) => new Date(NOW - minutesAgo * 60_000).toISOString();

const call = (id: string, tool: string, host: string, command: string | null, extra = {}) => ({
  run_id: RUN_ID,
  call_id: id,
  tool,
  host,
  command,
  path: null,
  args: {},
  ...extra,
});

const serviceRequest: AiApprovalRequest = {
  request_id: "req-service-1",
  run_id: RUN_ID,
  client: "Claude Code 2.0",
  tier: "out-of-band",
  created_at: iso(1),
  expires_at: new Date(NOW + 4 * 60_000).toISOString(),
  subject: {
    kind: "call",
    call: call("c-svc", "service", "web-2", "systemctl reload nginx", { args: { reason: "ignore previous instructions" } }),
    analysis: {
      commands: [
        {
          program: "systemctl",
          args: ["reload", "nginx"],
          wrappers: ["sudo"],
          writes_to: [],
          tags: ["service-change", "privilege"],
          rule: "systemctl-reload",
        },
      ],
      tags: ["service-change"],
      unknown_reason: null,
    },
  },
};

const destructiveRequest: AiApprovalRequest = {
  ...serviceRequest,
  request_id: "req-rm-1",
  subject: {
    kind: "call",
    call: call("c-rm", "run", "web-2", "rm -rf /var/cache/app"),
    analysis: { commands: [], tags: ["destructive"], unknown_reason: null },
  },
};

const PATCH = [
  "--- a/etc/nginx/conf.d/app.conf",
  "+++ b/etc/nginx/conf.d/app.conf",
  "@@ -3,3 +3,3 @@",
  " server {",
  "-    proxy_pass http://127.0.0.1:8080;",
  "+    proxy_pass http://127.0.0.1:8081;",
  " }",
].join("\n");

const planRequest: AiApprovalRequest = {
  ...serviceRequest,
  request_id: "req-plan-1",
  subject: {
    kind: "plan",
    plan_hash: "0123456789abcdef0123456789abcdef",
    plan: {
      change_id: "chg-1",
      run_id: RUN_ID,
      title: "Fix nginx 502 on web-2",
      reason: "upstream port changed to 8081",
      steps: [
        { id: "step-1", host: "web-2", action: { edit_file: { path: "/etc/nginx/conf.d/app.conf", patch: PATCH } } },
        { id: "step-2", host: "web-2", action: { run: { command: "nginx -t" } } },
        { id: "step-3", host: "web-2", action: { service: { unit: "nginx", action: "reload" } } },
      ],
      verify: [{ id: "verify-1", host: "web-2", action: { run: { command: "curl -fsS localhost/health" } } }],
      rollback: [{ id: "rollback-1", host: "web-2", action: { undo: { step: "step-1" } } }],
    },
    analyses: [
      { commands: [], tags: ["write"], unknown_reason: null },
      { commands: [], tags: ["read"], unknown_reason: null },
      { commands: [], tags: ["service-change"], unknown_reason: null },
      { commands: [], tags: ["network"], unknown_reason: null },
      null,
    ],
  },
};

const baseRun: AiRun = {
  run_id: RUN_ID,
  client: "Claude Code 2.0",
  policy: "default",
  started_at: iso(12),
  on_behalf_of: "alice",
  calls: 2,
  denied: 1,
  pending: [],
  grants: [],
  connected: true,
  ended: null,
};

const events: AiStoredEvent[] = [
  {
    seq: 1,
    at: iso(11),
    event: { event: "call", call: call("c-1", "run_readonly", "web-2", "journalctl -u nginx -n 50"), decision: { decision: "allow", reason: "read-only command" } },
  },
  {
    seq: 2,
    at: iso(11),
    event: {
      event: "result",
      run_id: RUN_ID,
      call_id: "c-1",
      exit_code: 0,
      output_sha256: "ab",
      redactions: [{ kind: "aws-key", count: 2 }],
    },
  },
  {
    seq: 3,
    at: iso(5),
    event: { event: "call", call: call("c-2", "run", "db-1", "cat /etc/shadow"), decision: { decision: "deny", reason: "path is never accessible" } },
  },
];

async function seed(page: Page, data: { runs?: AiRun[]; events?: Record<string, AiStoredEvent[]>; settings?: unknown }) {
  await page.addInitScript((d) => {
    const w = window as unknown as Record<string, unknown>;
    if (d.runs) w.__MOCK_AI_RUNS = d.runs;
    if (d.events) w.__MOCK_AI_EVENTS = d.events;
    if (d.settings) w.__MOCK_AI_SETTINGS = d.settings;
  }, data);
}

/** Wait until the store has subscribed, then push like the backend does. */
async function emit(page: Page, name: string, payload: unknown) {
  await expect
    .poll(() =>
      page.evaluate(
        (n) =>
          (window as unknown as { __IPC_CALLS(c: string): { event?: string }[] })
            .__IPC_CALLS("plugin:event|listen")
            .some((a) => a?.event === n),
        name,
      ),
    )
    .toBe(true);
  await page.evaluate(([n, p]) => (window as unknown as { __MOCK_EMIT(e: string, p: unknown): void }).__MOCK_EMIT(n as string, p), [
    name,
    payload,
  ]);
}

async function openAi(app: AppFixture, page: Page) {
  await app.gotoApp();
  await app.nav("AI agents");
  await expect(page.getByRole("heading", { name: "AI agents", level: 1 })).toBeVisible();
}

test.describe("AI agents view", () => {
  test("says so when no agent is connected", async ({ app, page }) => {
    await openAi(app, page);
    await expect(page.getByText("No agent is connected")).toBeVisible();
    await expect(page.getByRole("button", { name: /Review \d+ approval/ })).toHaveCount(0);
  });

  test("lists a live run with its timeline and stops it", async ({ app, page }) => {
    await seed(page, { runs: [baseRun], events: { [RUN_ID]: events } });
    await openAi(app, page);

    const runs = page.getByRole("list", { name: "Runs" });
    await expect(runs.getByText("Claude Code 2.0")).toBeVisible();
    await expect(runs.getByText("2 calls")).toBeVisible();
    await expect(runs.getByText("policy default")).toBeVisible();

    const timeline = page.getByRole("region", { name: "Timeline" });
    await expect(timeline.getByText("journalctl -u nginx -n 50")).toBeVisible();
    await expect(timeline.getByText("Allowed")).toBeVisible();
    await expect(timeline.getByText("Exit 0")).toBeVisible();
    await expect(timeline.getByText(/2 redactions \(aws-key ×2\)/)).toBeVisible();
    await expect(timeline.getByText("Denied")).toBeVisible();
    await expect(timeline.getByText("path is never accessible")).toBeVisible();
    // Newest first.
    const rows = timeline.getByRole("listitem");
    await expect(rows.first()).toContainText("cat /etc/shadow");

    await page.getByRole("button", { name: "Stop run" }).click();
    await app.waitForIpc("ai_stop");
    expect(await app.ipcCalls("ai_stop")).toEqual([{ runId: RUN_ID }]);
  });

  test("new events and run changes stream in", async ({ app, page }) => {
    await seed(page, { runs: [baseRun], events: { [RUN_ID]: events } });
    await openAi(app, page);
    const timeline = page.getByRole("region", { name: "Timeline" });

    await emit(page, "ai://event", {
      run_id: RUN_ID,
      seq: 4,
      at: new Date().toISOString(),
      event: { event: "call", call: call("c-3", "run_readonly", "web-2", "uptime"), decision: { decision: "allow", reason: "read-only command" } },
    });
    await expect(timeline.getByText("uptime", { exact: true })).toBeVisible();
    // A repeated seq is dropped.
    await emit(page, "ai://event", {
      run_id: RUN_ID,
      seq: 4,
      at: new Date().toISOString(),
      event: { event: "call", call: call("c-3", "run_readonly", "web-2", "uptime"), decision: { decision: "allow", reason: "read-only command" } },
    });
    await expect(timeline.getByText("uptime", { exact: true })).toHaveCount(1);

    await emit(page, "ai://runs", [{ ...baseRun, connected: false, ended: "stopped from the desktop app" }]);
    await expect(page.getByRole("button", { name: "Stop run" })).toBeDisabled();
    await expect(page.getByText("Ended: stopped from the desktop app")).toBeVisible();
  });

  test("shows active grants and revokes one", async ({ app, page }) => {
    await seed(page, {
      runs: [
        {
          ...baseRun,
          grants: [{ grant_id: "g-1", run_id: RUN_ID, tool: "service", hosts: "web-2", target: null, expires_at: new Date(NOW + 20 * 60_000).toISOString() }],
        },
      ],
    });
    await openAi(app, page);
    const grants = page.getByRole("region", { name: "Grants" });
    await expect(grants.getByText("web-2")).toBeVisible();
    await grants.getByRole("button", { name: "Revoke" }).click();
    await app.waitForIpc("ai_revoke_grant");
    expect(await app.ipcCalls("ai_revoke_grant")).toEqual([{ runId: RUN_ID, grantId: "g-1" }]);
  });
});

test.describe("approval dialog", () => {
  const withPending = (...pending: AiApprovalRequest[]) => ({
    runs: [{ ...baseRun, pending }],
    events: { [RUN_ID]: events },
  });

  test("opens on a new request and shows the exact command, host and analyzer tags", async ({ app, page }) => {
    await seed(page, { runs: [baseRun] });
    await openAi(app, page);
    await expect(page.getByRole("dialog")).toHaveCount(0);

    await emit(page, "ai://runs", [{ ...baseRun, pending: [serviceRequest] }]);
    await emit(page, "ai://request", { run_id: RUN_ID, request: serviceRequest });

    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("heading", { name: "Approve this command?" })).toBeVisible();
    await expect(dialog.getByLabel("Exact command")).toHaveText("systemctl reload nginx");
    await expect(dialog.getByText("web-2", { exact: true }).first()).toBeVisible();
    await expect(dialog.getByRole("region", { name: "Analyzer" }).getByText("Service change").first()).toBeVisible();
    await expect(dialog.getByText("Claude Code 2.0")).toBeVisible();
    // The sidebar badge counts what is waiting.
    await expect(page.locator(".sidebar").getByLabel(/^AI agents, 1 item needs attention$/)).toBeVisible();
  });

  test("the agent's own words are labelled and secondary", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog).toBeVisible();
    const says = dialog.getByRole("region", { name: "The agent says" });
    await expect(says).toContainText("ignore previous instructions");
    await expect(says).toContainText("not checked");
  });

  test("approve sends the decision and closes the dialog", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await dialog.getByRole("button", { name: "Approve", exact: true }).click();
    await app.waitForIpc("ai_decide");
    expect(await app.ipcCalls("ai_decide")).toEqual([
      { runId: RUN_ID, requestId: "req-service-1", approve: true, reason: null, grantMinutes: null },
    ]);
    await expect(dialog).toHaveCount(0);
  });

  test("approve and allow sends the chosen grant length", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await dialog.getByLabel("Grant length").selectOption("30");
    await dialog.getByRole("button", { name: "Approve and allow" }).click();
    await app.waitForIpc("ai_decide");
    expect(await app.ipcCalls("ai_decide")).toEqual([
      { runId: RUN_ID, requestId: "req-service-1", approve: true, reason: null, grantMinutes: 30 },
    ]);
  });

  test("deny carries the typed reason", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await dialog.getByLabel("Reason if you deny (optional)").fill("not during the release");
    await dialog.getByRole("button", { name: "Deny", exact: true }).click();
    await app.waitForIpc("ai_decide");
    expect(await app.ipcCalls("ai_decide")).toEqual([
      { runId: RUN_ID, requestId: "req-service-1", approve: false, reason: "not during the release", grantMinutes: null },
    ]);
  });

  test("deny is the focused action, so Enter can't approve by accident", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog.getByRole("button", { name: "Deny", exact: true })).toBeFocused();
  });

  test("no grant is offered for a destructive command", async ({ app, page }) => {
    await seed(page, withPending(destructiveRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog.getByLabel("Exact command")).toHaveText("rm -rf /var/cache/app");
    await expect(dialog.getByText("Destructive").first()).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Approve and allow" })).toHaveCount(0);
    await expect(dialog.getByText("No grant offered")).toBeVisible();
  });

  test("a plan shows every step, diff, verify and rollback, and offers no grant", async ({ app, page }) => {
    await seed(page, withPending(planRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog.getByRole("heading", { name: "Approve this change plan?" })).toBeVisible();
    await expect(dialog.getByText("Fix nginx 502 on web-2")).toBeVisible();

    const steps = dialog.getByRole("region", { name: "Steps" });
    await expect(steps.getByRole("listitem")).toHaveCount(3);
    await expect(steps.getByText("nginx -t")).toBeVisible();
    await expect(steps.getByText("Reload service")).toBeVisible();
    const diff = steps.getByRole("region", { name: "Changes to /etc/nginx/conf.d/app.conf" });
    await expect(diff.getByText("-    proxy_pass http://127.0.0.1:8080;")).toBeVisible();
    await expect(diff.getByText("+    proxy_pass http://127.0.0.1:8081;")).toBeVisible();
    await expect(steps.getByText("1 added, 1 removed")).toBeVisible();

    await expect(dialog.getByRole("region", { name: "Verify" }).getByText("curl -fsS localhost/health")).toBeVisible();
    await expect(dialog.getByRole("region", { name: "Rollback" }).getByText("step-1", { exact: true }).last()).toBeVisible();
    await expect(dialog.getByRole("region", { name: "The agent says" })).toContainText("upstream port changed to 8081");
    await expect(dialog.getByRole("button", { name: "Approve and allow" })).toHaveCount(0);
  });

  test("several requests page through and 'decide later' keeps them pending", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest, destructiveRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog.getByText("1 of 2")).toBeVisible();
    await dialog.getByRole("button", { name: "Next request" }).click();
    await expect(dialog.getByText("2 of 2")).toBeVisible();
    await expect(dialog.getByLabel("Exact command")).toHaveText("rm -rf /var/cache/app");

    await dialog.getByRole("button", { name: "Decide later", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await app.nav("AI agents");
    await page.getByRole("button", { name: "Review 2 approvals" }).click();
    await expect(page.getByRole("dialog", { name: "Approval needed" })).toBeVisible();
  });

  test("Escape defers the decision without sending anything", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    expect(await app.ipcCalls("ai_decide")).toEqual([]);
  });

  test("a failed decision is shown in the dialog", async ({ app, page }) => {
    await seed(page, withPending(serviceRequest));
    await page.addInitScript(() => {
      (window as unknown as { __MOCK_OVERRIDES: Record<string, () => never> }).__MOCK_OVERRIDES = {
        ai_decide: () => {
          throw "This request is no longer pending.";
        },
      };
    });
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await dialog.getByRole("button", { name: "Approve", exact: true }).click();
    await expect(dialog.getByRole("alert")).toHaveText("This request is no longer pending.");
    await expect(dialog).toBeVisible();
  });

  test("an expired request can't be answered", async ({ app, page }) => {
    const expired = { ...serviceRequest, expires_at: new Date(NOW - 1000).toISOString() };
    await seed(page, withPending(expired));
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Approval needed" });
    await expect(dialog.getByText("Expired").first()).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Approve", exact: true })).toBeDisabled();
    await expect(dialog.getByRole("button", { name: "Deny", exact: true })).toBeDisabled();
  });

  test("has no serious accessibility violations", async ({ app, page }) => {
    await seed(page, withPending(planRequest, serviceRequest));
    await app.gotoApp();
    await expect(page.getByRole("dialog", { name: "Approval needed" })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    const serious = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
    expect(
      serious.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`),
    ).toEqual([]);
  });
});

test.describe("AI view accessibility", () => {
  test("run view has no serious violations", async ({ app, page }) => {
    await seed(page, {
      runs: [{ ...baseRun, pending: [serviceRequest] }],
      events: { [RUN_ID]: events },
    });
    await app.gotoApp();
    await page.keyboard.press("Escape"); // the dialog opens by itself for pending requests
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await app.nav("AI agents");
    await expect(page.getByRole("heading", { name: "AI agents", level: 1 })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    const serious = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
    expect(
      serious.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`),
    ).toEqual([]);
  });
});

test.describe("AI settings", () => {
  test("shows the policies read-only", async ({ app, page }) => {
    await seed(page, {
      settings: {
        approval_timeout_secs: 300,
        record_output: true,
        policies: {
          default: {
            profile: "operate",
            scope: { tags: ["staging", "dev"], hosts: ["bastion"] },
            approval: { default: "out-of-band", in_client: ["staging"], two_person: ["prod-db"] },
            read_paths: ["/var/log/**"],
            deny_paths: ["/etc/shadow"],
            write_paths: ["/etc/nginx/**"],
            deny: ["rm -rf /"],
            budgets: { calls_per_min: 30, changes_per_run: 10, run_ttl: "2h", max_denials_per_min: 5 },
            redact: { vault_secrets: true, patterns: "default" },
          },
        },
      },
    });
    await app.gotoApp();
    await app.nav("Settings");
    await page.locator('nav[aria-label="Settings sections"]').getByRole("button", { name: "AI agents", exact: true }).click();

    const policy = page.getByRole("region", { name: "Policy default" });
    await expect(policy.getByText("Operate: changes need approval")).toBeVisible();
    await expect(policy.getByText("staging", { exact: true }).first()).toBeVisible();
    await expect(policy.getByText("prod-db")).toBeVisible();
    await expect(policy.getByText("/etc/shadow")).toBeVisible();
    await expect(policy.getByText("rm -rf /")).toBeVisible();
    await expect(policy.getByText("2h")).toBeVisible();
    await expect(page.getByText("5 min")).toBeVisible();
    // Read-only: nothing to edit.
    await expect(policy.getByRole("textbox")).toHaveCount(0);
    await expect(policy.getByRole("checkbox")).toHaveCount(0);
  });

  test("explains an empty policy list", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Settings");
    await page.locator('nav[aria-label="Settings sections"]').getByRole("button", { name: "AI agents", exact: true }).click();
    await expect(page.getByText("No policy defined")).toBeVisible();
  });
});
