<script lang="ts">
  import { onMount } from "svelte";
  import { Bot, OctagonX } from "lucide-svelte";
  import type { AiRun } from "$lib/types";
  import {
    getAiEvents,
    getAiRun,
    getAiRuns,
    openApproval,
    pendingApprovals,
    revokeAiGrant,
    stopAiRun,
  } from "$lib/stores/ai.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { formatDateTime, formatRelative } from "$lib/utils/timezone";
  import {
    buildTimeline,
    decisionLabel,
    formatClock,
    outcomeText,
    redactionSummary,
    shortId,
    stepHosts,
    toolLabel,
  } from "$lib/utils/ai";

  interface Props {
    timezone: string;
  }

  let { timezone }: Props = $props();

  let selectedId = $state<string | null>(null);
  let now = $state(Date.now());
  let stopping = $state(false);

  const runs = $derived(getAiRuns());
  const selected = $derived<AiRun | null>(getAiRun(selectedId) ?? runs[0] ?? null);
  const waiting = $derived(pendingApprovals());
  const selectedWaiting = $derived(selected ? waiting.filter((p) => p.run.run_id === selected.run_id) : []);
  // Newest first: the latest call is what a person watching wants to see.
  const timeline = $derived(selected ? buildTimeline(getAiEvents(selected.run_id)).reverse() : []);
  const live = $derived(selected !== null && selected.connected && selected.ended === null);

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  function status(run: AiRun): { label: string; live: boolean; title: string } {
    if (run.ended !== null) return { label: "Ended", live: false, title: run.ended };
    if (!run.connected) return { label: "Disconnected", live: false, title: "The run's process closed its socket" };
    return { label: "Active", live: true, title: "Connected" };
  }

  async function stop(run: AiRun) {
    stopping = true;
    try {
      await stopAiRun(run.run_id);
      notify("Stop sent. The run ends and its approvals are withdrawn.", "success");
    } catch (err) {
      notify(`Could not stop the run: ${err}`, "error");
    } finally {
      stopping = false;
    }
  }

  async function revoke(run: AiRun, grantId: string) {
    try {
      await revokeAiGrant(run.run_id, grantId);
      notify("Grant revoked", "success");
    } catch (err) {
      notify(`Could not revoke the grant: ${err}`, "error");
    }
  }

  function requestSummary(r: (typeof waiting)[number]["request"]): string {
    const s = r.subject;
    if (s.kind === "call") return s.call.command ?? `${toolLabel(s.call.tool)}${s.call.path ? ` ${s.call.path}` : ""}`;
    if (s.kind === "plan") return `${s.plan.title} (${s.plan.steps.length} ${s.plan.steps.length === 1 ? "step" : "steps"} on ${stepHosts(s.plan.steps).join(", ") || "no host"})`;
    return `Wider access for ${s.duration}`;
  }
</script>

<div class="view">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">AI agents</h1>
      <span class="text-sm tabular-nums text-muted">{runs.length} {runs.length === 1 ? "run" : "runs"}</span>
    </div>
    <div class="view-actions">
      {#if waiting.length > 0}
        <button type="button" class="btn btn-primary" onclick={() => openApproval()}>
          Review {waiting.length} {waiting.length === 1 ? "approval" : "approvals"}
        </button>
      {/if}
    </div>
  </header>

  <div class="view-body">
    {#if runs.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Bot size={18} /></div>
        <div class="empty-state-title">No agent is connected</div>
        <p class="empty-state-desc">Runs appear here while an agent works through <span class="mono">bssh ai mcp</span>.</p>
      </div>
    {:else}
      <div class="grid items-start gap-4 lg:grid-cols-[minmax(260px,340px)_minmax(0,1fr)]">
        <ul class="m-0 flex list-none flex-col gap-2 p-0" aria-label="Runs">
          {#each runs as run (run.run_id)}
            {@const st = status(run)}
            {@const isSelected = selected?.run_id === run.run_id}
            <li>
              <button
                type="button"
                class="card card-interactive w-full text-left {isSelected ? 'card-selected' : ''}"
                aria-pressed={isSelected}
                onclick={() => (selectedId = run.run_id)}
              >
                <span class="flex items-center justify-between gap-2">
                  <span class="truncate text-sm font-medium text-primary" title={run.client}>{run.client || "Unknown agent"}</span>
                  {#if run.pending.length > 0}
                    <span class="badge badge-warning">{run.pending.length} pending</span>
                  {/if}
                </span>
                <span class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted">
                  <span class="flex items-center gap-1.5" title={st.title}>
                    <span class="status-dot status-dot-sm {st.live ? 'status-dot-running' : 'status-dot-offline'}" aria-hidden="true"></span>
                    {st.label}
                  </span>
                  <span>policy {run.policy}</span>
                  <span title={formatDateTime(run.started_at, timezone)}>started {formatRelative(run.started_at, now)}</span>
                  <span class="tabular-nums">{run.calls} {run.calls === 1 ? "call" : "calls"}</span>
                </span>
              </button>
            </li>
          {/each}
        </ul>

        {#if selected}
          {@const st = status(selected)}
          <div class="flex min-w-0 flex-col gap-4">
            <section class="panel" aria-label="Run">
              <div class="panel-header">
                <div class="flex min-w-0 items-center gap-2">
                  <h2 class="panel-title truncate" title={selected.client}>{selected.client || "Unknown agent"}</h2>
                  <span class="badge {st.live ? 'badge-accent' : 'badge-neutral'}" title={st.title}>{st.label}</span>
                </div>
                <button
                  type="button"
                  class="btn btn-danger"
                  disabled={!live || stopping}
                  title={live ? "End this run now" : "The run is no longer connected"}
                  onclick={() => stop(selected)}
                >
                  <OctagonX size={14} />
                  Stop run
                </button>
              </div>
              <dl class="panel-body m-0 grid grid-cols-2 gap-x-6 gap-y-3 text-sm sm:grid-cols-4">
                <div>
                  <dt class="type-meta">Policy</dt>
                  <dd class="m-0 text-primary">{selected.policy}</dd>
                </div>
                <div>
                  <dt class="type-meta">Started</dt>
                  <dd class="m-0 text-primary" title={formatDateTime(selected.started_at, timezone)}>{formatRelative(selected.started_at, now)}</dd>
                </div>
                <div>
                  <dt class="type-meta">Calls</dt>
                  <dd class="m-0 tabular-nums text-primary">{selected.calls}{selected.denied > 0 ? `, ${selected.denied} denied` : ""}</dd>
                </div>
                <div>
                  <dt class="type-meta">Run</dt>
                  <dd class="mono m-0 text-primary" title={selected.run_id}>{shortId(selected.run_id)}</dd>
                </div>
                {#if selected.on_behalf_of}
                  <div>
                    <dt class="type-meta">On behalf of</dt>
                    <dd class="m-0 text-primary">{selected.on_behalf_of}</dd>
                  </div>
                {/if}
              </dl>
              {#if selected.ended}
                <p class="type-meta m-0 px-4 pb-3">Ended: {selected.ended}</p>
              {/if}
            </section>

            {#if selectedWaiting.length > 0}
              <section class="panel" aria-label="Waiting for approval">
                <div class="panel-header"><h2 class="panel-title">Waiting for approval</h2></div>
                <ul class="m-0 flex list-none flex-col p-0">
                  {#each selectedWaiting as item (item.request.request_id)}
                    <li class="flex items-center justify-between gap-3 border-t border-border-subtle px-4 py-2 first:border-t-0">
                      <span class="mono min-w-0 flex-1 truncate text-primary" title={requestSummary(item.request)}>
                        {requestSummary(item.request)}
                      </span>
                      <button type="button" class="btn btn-secondary btn-sm" onclick={() => openApproval(item.request.request_id)}>
                        Review
                      </button>
                    </li>
                  {/each}
                </ul>
              </section>
            {/if}

            {#if selected.grants.length > 0}
              <section class="panel" aria-label="Grants">
                <div class="panel-header"><h2 class="panel-title">Active grants</h2></div>
                <ul class="m-0 flex list-none flex-col p-0">
                  {#each selected.grants as grant (grant.grant_id)}
                    <li class="flex items-center justify-between gap-3 border-t border-border-subtle px-4 py-2 first:border-t-0">
                      <span class="flex min-w-0 flex-wrap items-center gap-1.5 text-sm text-primary">
                        {toolLabel(grant.tool)} on <span class="tag">{grant.hosts}</span>
                        {#if grant.target}<span class="mono break-all text-xs">{grant.target}</span>{/if}
                        <span class="type-meta" title={formatDateTime(grant.expires_at, timezone)}>
                          until {new Date(grant.expires_at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
                        </span>
                      </span>
                      <button
                        type="button"
                        class="btn btn-ghost btn-sm"
                        disabled={!live}
                        onclick={() => revoke(selected, grant.grant_id)}
                      >
                        Revoke
                      </button>
                    </li>
                  {/each}
                </ul>
              </section>
            {/if}

            <section class="panel" aria-label="Timeline">
              <div class="panel-header">
                <h2 class="panel-title">Timeline</h2>
                <span class="type-meta">Newest first</span>
              </div>
              {#if timeline.length === 0}
                <p class="type-meta m-0 px-4 py-3">Nothing yet. Calls show up here as the agent makes them.</p>
              {:else}
                <ol class="m-0 flex list-none flex-col p-0">
                  {#each timeline as entry (entry.key)}
                    <li class="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-3 border-t border-border-subtle px-4 py-2.5 first:border-t-0">
                      <time class="mono tabular-nums text-muted" datetime={entry.at} title={formatDateTime(entry.at, timezone)}>
                        {formatClock(entry.at)}
                      </time>
                      <div class="flex min-w-0 flex-col gap-1">
                        {#if entry.kind === "call"}
                          {@const d = decisionLabel(entry.decision)}
                          <div class="flex flex-wrap items-center gap-1.5">
                            <span class="tag">{toolLabel(entry.call.tool)}</span>
                            {#if entry.call.host}<span class="tag">{entry.call.host}</span>{/if}
                            <span class="badge {d.badge}">{d.label}</span>
                            {#if entry.result}
                              <span class="badge {entry.result.exit_code === 0 ? 'badge-neutral' : 'badge-error'} tabular-nums">
                                {entry.result.exit_code === null ? "No exit code" : `Exit ${entry.result.exit_code}`}
                              </span>
                            {/if}
                          </div>
                          {#if entry.call.command || entry.call.path}
                            <span class="mono truncate text-primary" title={entry.call.command ?? entry.call.path}>
                              {entry.call.command ?? entry.call.path}
                            </span>
                          {/if}
                          <span class="type-meta break-words">{entry.decision.reason}</span>
                          {#if entry.result && redactionSummary(entry.result.redactions)}
                            <span class="type-meta">Redacted before the agent saw it: {redactionSummary(entry.result.redactions)}</span>
                          {/if}
                        {:else if entry.kind === "approval"}
                          <span class="text-sm text-primary">{outcomeText(entry.outcome)}</span>
                          <span class="type-meta">Request <span class="mono">{shortId(entry.requestId)}</span></span>
                        {:else if entry.kind === "change"}
                          <span class="text-sm text-primary">
                            Change <span class="mono">{shortId(entry.changeId)}</span>
                            {entry.applied ? "applied" : "not applied"}
                          </span>
                          {#if entry.undoPoint}
                            <span class="type-meta">Undo point <span class="mono">{entry.undoPoint}</span></span>
                          {/if}
                        {:else}
                          <span class="text-sm {entry.tone === 'warning' ? 'text-warning' : 'text-secondary'}">
                            {#if entry.tone === "warning"}<span class="sr-only">Warning: </span>{/if}{entry.text}
                          </span>
                        {/if}
                      </div>
                    </li>
                  {/each}
                </ol>
              {/if}
            </section>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
