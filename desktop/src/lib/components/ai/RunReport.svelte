<script lang="ts">
  import type { Snippet } from "svelte";
  import type { AiReport, AiReportApproval, AiReportChange, AiReportStep } from "$lib/types";
  import { formatDateTime } from "$lib/utils/timezone";
  import { decisionLabel, formatClock, formatDuration, redactionSummary, shortId, toolLabel } from "$lib/utils/ai";

  interface Props {
    report: AiReport;
    timezone: string;
    /** Controls shown at the right of the report's header (the Download menu). */
    actions?: Snippet;
  }

  let { report, timezone, actions }: Props = $props();

  const duration = $derived(formatDuration(report.started, report.ended));
  const totals = $derived({
    allowed: report.calls.filter((c) => c.decision === "allow").length,
    asked: report.calls.filter((c) => c.decision === "ask").length,
    denied: report.calls.filter((c) => c.decision === "deny").length,
    approved: report.approvals.filter((a) => a.outcome === "approved").length,
    approvalsDenied: report.approvals.filter((a) => a.outcome === "denied").length,
    expired: report.approvals.filter((a) => a.outcome === "expired").length,
  });
  const redactions = $derived(redactionSummary(report.redactions));
  const deniedCalls = $derived(report.calls.filter((c) => c.decision === "deny"));

  const PHASES: { phase: AiReportStep["phase"]; label: string }[] = [
    { phase: "step", label: "Steps" },
    { phase: "verify", label: "Checks" },
    { phase: "undo", label: "Undo" },
  ];

  function plural(n: number, one: string, many: string): string {
    return `${n} ${n === 1 ? one : many}`;
  }

  function stamp(iso: string | null | undefined): string {
    return iso ? formatDateTime(iso, timezone) : "";
  }

  function changeTitle(change: AiReportChange): string {
    return change.title || `plan ${shortId(change.plan_hash)}`;
  }

  function changeState(change: AiReportChange): { label: string; badge: string } {
    if (change.undone) return { label: "Undone", badge: "badge-neutral" };
    if (change.applied) return { label: "Applied", badge: "badge-success" };
    const failed = change.steps.find((s) => s.ok === false);
    if (failed) return { label: `Stopped at a failed ${failed.phase === "verify" ? "check" : "step"}`, badge: "badge-error" };
    return { label: "Not applied", badge: "badge-neutral" };
  }

  function stepResult(step: AiReportStep): { label: string; badge: string; title: string } {
    if (step.ok === true) {
      return {
        label: step.exit_code !== null && step.exit_code !== 0 ? `OK, exit ${step.exit_code}` : "OK",
        badge: "badge-success",
        title: "",
      };
    }
    if (step.ok === false) {
      const label = step.exit_code !== null ? `Failed, exit ${step.exit_code}` : "Failed";
      return { label, badge: "badge-error", title: step.error ?? "" };
    }
    return { label: "Not run", badge: "badge-neutral", title: "" };
  }

  function stepsOfPhase(change: AiReportChange, phase: AiReportStep["phase"]): AiReportStep[] {
    return change.steps.filter((s) => s.phase === phase);
  }

  function outcomeBadge(outcome: AiReportApproval["outcome"]): { label: string; badge: string } {
    switch (outcome) {
      case "approved":
        return { label: "Approved", badge: "badge-success" };
      case "denied":
        return { label: "Denied", badge: "badge-error" };
      case "expired":
        return { label: "Expired", badge: "badge-neutral" };
    }
  }

  function callTarget(c: { command: string | null; path: string | null }): string {
    return c.command ?? c.path ?? "";
  }

  /** `by ana via desktop at …`; empty when nothing is known. */
  function approvedText(change: AiReportChange): string {
    const parts: string[] = [];
    if (change.approved_by) parts.push(`by ${change.approved_by}`);
    if (change.approved_channel && change.approved_channel !== "in-band") parts.push(`via ${change.approved_channel}`);
    if (change.approved_at) parts.push(`at ${stamp(change.approved_at)}`);
    return parts.join(" ");
  }
</script>

<div class="flex min-w-0 flex-col gap-4">
  <section class="panel" aria-label="Run summary">
    <div class="panel-header">
      <div class="flex min-w-0 items-center gap-2">
        <h2 class="panel-title truncate" title={report.client ?? ""}>{report.client || "Unknown agent"}</h2>
        {#if report.ended}
          <span class="badge badge-neutral">Ended</span>
        {:else}
          <span class="badge badge-warning" title="The audit log has no end for this run">No end recorded</span>
        {/if}
      </div>
      {#if actions}
        <div class="flex items-center gap-2">{@render actions()}</div>
      {/if}
    </div>
    <dl class="panel-body m-0 grid grid-cols-2 gap-x-6 gap-y-3 text-sm sm:grid-cols-4">
      <div>
        <dt class="type-meta">Agent</dt>
        <dd class="m-0 text-primary">{report.client || "Unknown agent"}</dd>
      </div>
      {#if report.on_behalf_of}
        <div>
          <dt class="type-meta">On behalf of</dt>
          <dd class="m-0 break-words text-primary">{report.on_behalf_of}</dd>
        </div>
      {/if}
      <div>
        <dt class="type-meta">Policy</dt>
        <dd class="m-0 text-primary" title={report.policy_hash ?? ""}>{report.policy || "Unknown"}</dd>
      </div>
      <div>
        <dt class="type-meta">Started</dt>
        <dd class="m-0 text-primary">{report.started ? stamp(report.started) : "Not recorded"}</dd>
      </div>
      <div>
        <dt class="type-meta">Ended</dt>
        <dd class="m-0 text-primary">
          {#if report.ended}
            {stamp(report.ended)}{#if report.end_reason}{" "}<span class="text-muted">({report.end_reason})</span>{/if}
          {:else}
            No end recorded
          {/if}
        </dd>
      </div>
      <div>
        <dt class="type-meta">Duration</dt>
        <dd class="m-0 tabular-nums text-primary">{duration || "Unknown"}</dd>
      </div>
      <div>
        <dt class="type-meta">Run</dt>
        <dd class="mono m-0 text-primary" title={report.run_id}>{shortId(report.run_id)}</dd>
      </div>
      <div class="col-span-2 sm:col-span-4">
        <dt class="type-meta">Hosts</dt>
        <dd class="m-0 flex flex-wrap gap-1.5 text-primary">
          {#each report.hosts as host (host)}
            <span class="tag">{host}</span>
          {:else}
            <span class="text-muted">No host touched</span>
          {/each}
        </dd>
      </div>
    </dl>
    <p class="type-meta m-0 border-t border-border-subtle px-4 py-3 tabular-nums">
      {plural(report.calls.length, "call", "calls")}: {totals.allowed} allowed, {totals.asked} needed approval,
      {totals.denied} denied. Approvals: {totals.approved} approved, {totals.approvalsDenied} denied,
      {totals.expired} expired. {plural(report.changes.length, "change", "changes")}.
    </p>
  </section>

  <section class="panel" aria-label="Changes">
    <div class="panel-header">
      <h2 class="panel-title">Changes</h2>
      <span class="type-meta tabular-nums">{report.changes.length}</span>
    </div>
    {#if report.changes.length === 0}
      <p class="type-meta m-0 px-4 py-3">None. The agent made no changes in this run.</p>
    {:else}
      <ul class="m-0 flex list-none flex-col p-0" aria-label="Changes">
        {#each report.changes as change (change.change_id + change.seq)}
          {@const state = changeState(change)}
          <li class="flex flex-col gap-3 border-t border-border-subtle px-4 py-3 first:border-t-0">
            <div class="flex flex-wrap items-center gap-2">
              <span class="badge {state.badge}">{state.label}</span>
              <h3 class="m-0 min-w-0 break-words text-sm font-medium text-primary">{changeTitle(change)}</h3>
              <span class="mono text-muted" title={change.change_id}>{shortId(change.change_id)}</span>
            </div>
            <dl class="m-0 grid grid-cols-1 gap-x-6 gap-y-1 text-xs sm:grid-cols-2">
              <div>
                <dt class="inline text-muted">Time</dt>
                <dd class="m-0 inline text-primary"> {stamp(change.ts)}</dd>
              </div>
              {#if approvedText(change)}
                <div>
                  <dt class="inline text-muted">Approved</dt>
                  <dd class="m-0 inline text-primary"> {approvedText(change)}</dd>
                </div>
              {/if}
              {#if change.undo_point}
                <div>
                  <dt class="inline text-muted">Undo point</dt>
                  <dd class="mono m-0 inline text-primary"> {change.undo_point}</dd>
                </div>
              {/if}
              {#if change.steps_ok}
                <div>
                  <dt class="inline text-muted">Steps ok</dt>
                  <dd class="m-0 inline tabular-nums text-primary"> {change.steps_ok[0]}/{change.steps_ok[1]}</dd>
                </div>
              {/if}
              {#if change.checks_ok}
                <div>
                  <dt class="inline text-muted">Checks ok</dt>
                  <dd class="m-0 inline tabular-nums text-primary"> {change.checks_ok[0]}/{change.checks_ok[1]}</dd>
                </div>
              {/if}
            </dl>
            {#if change.reason}
              <p class="m-0 rounded-md border border-border bg-surface-input px-3 py-2 text-xs text-secondary">
                <span class="section-label block">The agent says</span>
                <span class="break-words">{change.reason}</span>
              </p>
            {/if}

            {#each PHASES as group (group.phase)}
              {@const phaseSteps = stepsOfPhase(change, group.phase)}
              {#if phaseSteps.length > 0}
                <div class="flex flex-col gap-2">
                  <h4 class="section-label m-0">{group.label}</h4>
                  <ol class="m-0 flex list-none flex-col gap-2 p-0" aria-label={group.label}>
                    {#each phaseSteps as step (step.id + step.phase)}
                      {@const result = stepResult(step)}
                      <li class="flex flex-col gap-1.5 rounded-lg border border-border bg-panel p-3">
                        <div class="flex flex-wrap items-center gap-1.5">
                          <span class="tag font-mono">{step.id}</span>
                          {#if step.host}<span class="tag">{step.host}</span>{/if}
                          <span class="badge {result.badge}" title={result.title}>{result.label}</span>
                        </div>
                        <span class="mono break-all text-primary">{step.action}</span>
                        {#if step.error}
                          <span class="type-meta break-words text-error">{step.error}</span>
                        {/if}
                        {#if step.output}
                          <details>
                            <summary class="type-meta cursor-pointer">Output</summary>
                            <pre class="code-block mt-2 whitespace-pre-wrap break-words">{step.output}</pre>
                          </details>
                        {/if}
                      </li>
                    {/each}
                  </ol>
                </div>
              {/if}
            {/each}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="panel" aria-label="Denied by policy">
    <div class="panel-header">
      <h2 class="panel-title">Denied by policy</h2>
      <span class="type-meta tabular-nums">{deniedCalls.length}</span>
    </div>
    {#if deniedCalls.length === 0 && report.denials.length === 0}
      <p class="type-meta m-0 px-4 py-3">None.</p>
    {:else}
      {#if deniedCalls.length > 0}
        <ul class="m-0 flex list-none flex-col p-0" aria-label="Denied calls">
          {#each deniedCalls as c (c.seq)}
            <li class="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-3 border-t border-border-subtle px-4 py-2.5 first:border-t-0">
              <time class="mono tabular-nums text-muted" datetime={c.ts} title={stamp(c.ts)}>{formatClock(c.ts)}</time>
              <div class="flex min-w-0 flex-col gap-1">
                <div class="flex flex-wrap items-center gap-1.5">
                  <span class="tag">{toolLabel(c.tool)}</span>
                  {#if c.host}<span class="tag">{c.host}</span>{/if}
                </div>
                {#if callTarget(c)}
                  <span class="mono break-all text-primary">{callTarget(c)}</span>
                {/if}
                <span class="type-meta break-words">{c.reason}</span>
              </div>
            </li>
          {/each}
        </ul>
      {:else}
        <ul class="m-0 flex list-none flex-col p-0" aria-label="Denied calls">
          {#each report.denials as line, i (i)}
            <li class="break-words border-t border-border-subtle px-4 py-2.5 text-sm text-primary first:border-t-0">{line}</li>
          {/each}
        </ul>
      {/if}
    {/if}
  </section>

  <section class="panel" aria-label="Approvals">
    <div class="panel-header">
      <h2 class="panel-title">Approvals</h2>
      <span class="type-meta tabular-nums">{report.approvals.length}</span>
    </div>
    {#if report.approvals.length === 0}
      <p class="type-meta m-0 px-4 py-3">None. No request needed a person's answer.</p>
    {:else}
      <ul class="m-0 flex list-none flex-col p-0" aria-label="Approvals">
        {#each report.approvals as a (a.seq)}
          {@const o = outcomeBadge(a.outcome)}
          <li class="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-3 border-t border-border-subtle px-4 py-2.5 first:border-t-0">
            <time class="mono tabular-nums text-muted" datetime={a.ts} title={stamp(a.ts)}>{formatClock(a.ts)}</time>
            <div class="flex min-w-0 flex-col gap-1">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="badge {o.badge}">{o.label}</span>
                {#if a.by}<span class="text-sm text-primary">by {a.by}</span>{/if}
                {#if a.channel}<span class="tag">{a.channel}</span>{/if}
                <span class="mono text-muted" title={a.request_id}>{shortId(a.request_id)}</span>
              </div>
              {#if a.reason}<span class="type-meta break-words">{a.reason}</span>{/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="panel" aria-label="All calls">
    <div class="panel-header">
      <h2 class="panel-title">All calls</h2>
      <span class="type-meta">Oldest first</span>
    </div>
    {#if report.calls.length === 0}
      <p class="type-meta m-0 px-4 py-3">None. The agent made no tool calls.</p>
    {:else}
      <ol class="m-0 flex list-none flex-col p-0" aria-label="Calls">
        {#each report.calls as c (c.seq)}
          {@const d = decisionLabel(c)}
          {@const red = redactionSummary(c.redactions)}
          <li class="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-3 border-t border-border-subtle px-4 py-2.5 first:border-t-0">
            <time class="mono tabular-nums text-muted" datetime={c.ts} title={stamp(c.ts)}>{formatClock(c.ts)}</time>
            <div class="flex min-w-0 flex-col gap-1">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="tag">{toolLabel(c.tool)}</span>
                {#if c.host}<span class="tag">{c.host}</span>{/if}
                <span class="badge {d.badge}" title={c.reason}>{d.label}</span>
                {#if c.exit_code !== null}
                  <span class="badge {c.exit_code === 0 ? 'badge-neutral' : 'badge-error'} tabular-nums">Exit {c.exit_code}</span>
                {/if}
              </div>
              {#if callTarget(c)}
                <span class="mono truncate text-primary" title={callTarget(c)}>{callTarget(c)}</span>
              {/if}
              {#if c.reason}
                <span class="type-meta truncate" title={c.reason}>{c.reason}</span>
              {/if}
              {#if red}
                <span class="type-meta">Redacted before the agent saw it: {red}</span>
              {/if}
            </div>
          </li>
        {/each}
      </ol>
    {/if}
  </section>

  {#if redactions || report.circuit_breaker.length > 0}
    <section class="panel" aria-label="Safety">
      <div class="panel-header"><h2 class="panel-title">Safety</h2></div>
      <div class="panel-body flex flex-col gap-3 text-sm">
        {#if redactions}
          <div>
            <div class="type-meta">Redactions</div>
            <p class="m-0 text-primary">{redactions}</p>
          </div>
        {/if}
        {#if report.circuit_breaker.length > 0}
          <div>
            <div class="type-meta">Circuit breaker</div>
            <ul class="m-0 list-disc pl-5 text-primary" aria-label="Circuit breaker reasons">
              {#each report.circuit_breaker as reason, i (i)}
                <li class="break-words">{reason}</li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    </section>
  {/if}

  <section class="panel" aria-label="Recordings">
    <div class="panel-header">
      <h2 class="panel-title">Recordings</h2>
      <span class="type-meta tabular-nums">{report.recordings.length}</span>
    </div>
    <div class="panel-body text-sm">
      {#if report.recordings.length === 0}
        <p class="m-0 text-muted">No session was recorded for this run.</p>
      {:else}
        <p class="m-0 text-primary">
          {plural(report.recordings.length, "recording", "recordings")}. Replay from a terminal:
        </p>
        <pre class="code-block mt-2 whitespace-pre-wrap break-words">bssh ai replay {shortId(report.run_id)}</pre>
      {/if}
    </div>
  </section>
</div>
