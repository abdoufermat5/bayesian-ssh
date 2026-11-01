<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ChevronDown, Bot, Download, ExternalLink, FileText, FolderOpen, OctagonX, Play, Printer, RefreshCw, Search, Trash2, X } from "lucide-svelte";
  import type { AiPruneOutcome, AiReport, AiReportFormat, AiReportSummary, AiRun } from "$lib/types";
  import {
    clearAiReportTarget,
    getAiEvents,
    getAiMode,
    getAiRun,
    getAiRuns,
    openApproval,
    pendingAiReportTarget,
    pendingApprovals,
    resumeAiRun,
    revokeAiGrant,
    setAiMode,
    showAiReports,
    stopAiRun,
  } from "$lib/stores/ai.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import RunReport from "./ai/RunReport.svelte";
  import CleanupDialog from "./ai/CleanupDialog.svelte";
  import { formatDateTime, formatRelative } from "$lib/utils/timezone";
  import {
    buildTimeline,
    decisionLabel,
    filterReports,
    formatClock,
    outcomeText,
    REPORT_FILTERS,
    type ReportFilter,
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
  let resuming = $state(false);
  // Shared with the sidebar's "AI reports" entry, the palette and the 9 key.
  const mode = $derived(getAiMode());

  let reports = $state.raw<AiReportSummary[]>([]);
  let reportsLoading = $state(false);
  let reportsError = $state<string | null>(null);
  /** Chosen run in the list; null follows the newest one. */
  let reportId = $state<string | null>(null);
  let reportCache = $state.raw<Record<string, AiReport>>({});
  let reportLoadingId = $state<string | null>(null);
  let reportError = $state<string | null>(null);
  let downloadMenuOpen = $state(false);
  let exporting = $state(false);
  let viewingHtml = $state(false);
  /** The report file just saved: offered to open or show in the folder. */
  let savedFile = $state<{ path: string; runId: string } | null>(null);
  let reportQuery = $state("");
  let reportFilter = $state<ReportFilter>("all");
  let listRequest = 0;
  /** Cleanup dialog: closed (null), or open with an optional preselected run. */
  let cleanup = $state<{ runId: string | null } | null>(null);

  const FORMATS: { id: AiReportFormat; label: string }[] = [
    { id: "txt", label: "Text (.txt)" },
    { id: "md", label: "Markdown (.md)" },
    { id: "html", label: "HTML (.html)" },
    { id: "pdf", label: "PDF (.pdf)" },
    { id: "json", label: "JSON (.json)" },
  ];

  const runs = $derived(getAiRuns());
  const liveIds = $derived(new Set(runs.filter((r) => r.connected && r.ended === null).map((r) => r.run_id)));
  const shownReports = $derived(filterReports(reports, reportQuery, reportFilter));
  const filtering = $derived(reportQuery.trim() !== "" || reportFilter !== "all");
  const reportRunId = $derived(reportId ?? reports[0]?.run_id ?? null);
  const shownReport = $derived(reportRunId ? (reportCache[reportRunId] ?? null) : null);
  const reportLoading = $derived(reportRunId !== null && reportLoadingId === reportRunId);
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

  // Fetch the chosen run's report once; Refresh clears the cache.
  $effect(() => {
    const id = mode === "reports" ? reportRunId : null;
    if (id) untrack(() => void loadReport(id));
  });

  // Entering the reports (toggle, sidebar, key, palette) loads the list.
  $effect(() => {
    if (mode === "reports") untrack(() => void loadReports());
  });

  // A deep link (openReport, or another view) picks the run to show.
  $effect(() => {
    const target = pendingAiReportTarget();
    if (!target) return;
    reportId = target;
    reportQuery = "";
    reportFilter = "all";
    clearAiReportTarget();
  });

  function setMode(next: "live" | "reports") {
    if (next === mode) return;
    downloadMenuOpen = false;
    setAiMode(next);
  }

  function openReport(runId: string) {
    showAiReports(runId);
  }

  async function loadReports() {
    const request = ++listRequest;
    reportsLoading = true;
    reportsError = null;
    try {
      const list = await invoke<AiReportSummary[]>("ai_reports", { limit: 200 });
      if (request !== listRequest) return;
      reports = list;
    } catch (err) {
      if (request !== listRequest) return;
      reportsError = String(err);
      notify(`Could not load the reports: ${err}`, "error");
    } finally {
      if (request === listRequest) reportsLoading = false;
    }
  }

  async function loadReport(runId: string, force = false) {
    if (!force && reportCache[runId]) return;
    reportLoadingId = runId;
    reportError = null;
    try {
      const full = await invoke<AiReport>("ai_report", { runId });
      reportCache = { ...reportCache, [runId]: full };
    } catch (err) {
      if (reportRunId === runId) {
        reportError = String(err);
        notify(`Could not load the report: ${err}`, "error");
      }
    } finally {
      if (reportLoadingId === runId) reportLoadingId = null;
    }
  }

  async function refreshReports() {
    reportCache = {};
    const current = reportRunId;
    await loadReports();
    // The effect refetches when the chosen run changed; otherwise do it here.
    if (current && current === reportRunId) await loadReport(current, true);
  }

  function onPruned(_outcome: AiPruneOutcome) {
    reportId = null;
    void refreshReports();
  }

  async function download(format: AiReportFormat) {
    downloadMenuOpen = false;
    const runId = reportRunId;
    if (!runId) return;
    exporting = true;
    try {
      const path = await invoke<string | null>("ai_report_export", { runId, format });
      if (path) savedFile = { path, runId };
    } catch (err) {
      notify(`Could not save the report: ${err}`, "error");
    } finally {
      exporting = false;
    }
  }

  async function viewHtml() {
    const runId = reportRunId;
    if (!runId) return;
    viewingHtml = true;
    try {
      await invoke<string>("ai_report_view_html", { runId });
      notify("Opened in your browser. Use its Print command to print or save as PDF.", "success");
    } catch (err) {
      notify(`Could not open the report: ${err}`, "error");
    } finally {
      viewingHtml = false;
    }
  }

  async function filePath(command: "ai_open_path" | "ai_reveal_path") {
    if (!savedFile) return;
    try {
      await invoke(command, { path: savedFile.path });
    } catch (err) {
      notify(`${command === "ai_open_path" ? "Could not open" : "Could not show"} the file: ${err}`, "error");
    }
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (downloadMenuOpen && e.key === "Escape") {
      e.stopPropagation();
      downloadMenuOpen = false;
    }
  }

  function status(run: AiRun): { label: string; live: boolean; title: string } {
    if (run.ended !== null) return { label: "Ended", live: false, title: run.ended };
    if (!run.connected) return { label: "Disconnected", live: false, title: "The run's process closed its socket" };
    if (run.paused) return { label: "Paused", live: false, title: run.pause_reason ?? "Paused by the circuit breaker" };
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

  async function resume(run: AiRun) {
    resuming = true;
    try {
      await resumeAiRun(run.run_id);
      notify("Resume sent. The run can make tool calls again.", "success");
    } catch (err) {
      notify(`Could not resume the run: ${err}`, "error");
    } finally {
      resuming = false;
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

{#snippet reportActions()}
  <button
    type="button"
    class="btn btn-secondary"
    disabled={viewingHtml || reportRunId === null}
    title="Open this report as a web page in your browser, ready to print"
    onclick={viewHtml}
  >
    {#if viewingHtml}<span class="spinner size-3" aria-hidden="true"></span>{:else}<Printer size={14} />{/if}
    View / Print
  </button>
  <div class="relative">
    <button
      type="button"
      class="btn btn-secondary"
      aria-haspopup="menu"
      aria-expanded={downloadMenuOpen}
      disabled={exporting || reportRunId === null}
      onclick={() => (downloadMenuOpen = !downloadMenuOpen)}
    >
      {#if exporting}<span class="spinner size-3" aria-hidden="true"></span>{:else}<Download size={14} />{/if}
      Download
      <ChevronDown size={14} />
    </button>
    {#if downloadMenuOpen}
      <div class="fixed inset-0 z-40" role="presentation" onclick={() => (downloadMenuOpen = false)}></div>
      <div class="popover absolute right-0 top-full mt-1 w-44" role="menu" aria-label="Download format">
        {#each FORMATS as f (f.id)}
          <button type="button" class="menu-item" role="menuitem" onclick={() => download(f.id)}>
            {f.label}
          </button>
        {/each}
      </div>
    {/if}
  </div>
  <button type="button" class="btn btn-secondary" disabled={reportRunId === null} onclick={() => (cleanup = { runId: reportRunId })}>
    <Trash2 size={14} />
    Delete…
  </button>
{/snippet}

<svelte:window onkeydown={onWindowKeydown} />

<div class="view">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">AI agents</h1>
      <span class="text-sm tabular-nums text-muted">
        {#if mode === "reports"}
          {#if filtering}{shownReports.length} of {/if}{reports.length} {reports.length === 1 ? "run" : "runs"} in the audit log
        {:else}
          {runs.length} {runs.length === 1 ? "run" : "runs"}
        {/if}
      </span>
      <div class="segmented ml-1 self-center" role="group" aria-label="AI agents view">
        <button
          type="button"
          class="segmented-item {mode === 'live' ? 'segmented-item-active' : ''}"
          aria-pressed={mode === "live"}
          onclick={() => setMode("live")}
        >
          Live
        </button>
        <button
          type="button"
          class="segmented-item {mode === 'reports' ? 'segmented-item-active' : ''}"
          aria-pressed={mode === "reports"}
          onclick={() => setMode("reports")}
        >
          Reports
        </button>
      </div>
    </div>
    <div class="view-actions">
      {#if mode === "reports"}
        <button type="button" class="btn btn-ghost" onclick={refreshReports} disabled={reportsLoading || reportLoading}>
          <RefreshCw size={14} class={reportsLoading || reportLoading ? "animate-spin" : ""} />
          Refresh
        </button>
      {/if}
      <button type="button" class="btn btn-ghost" onclick={() => (cleanup = { runId: null })}>
        <Trash2 size={14} />
        Clean up…
      </button>
      {#if waiting.length > 0}
        <button type="button" class="btn btn-primary" onclick={() => openApproval()}>
          Review {waiting.length} {waiting.length === 1 ? "approval" : "approvals"}
        </button>
      {/if}
    </div>
  </header>

  {#if mode === "reports" && reports.length > 0}
    <div class="view-toolbar flex-wrap gap-2">
      <label class="search-box w-64 lg:w-80">
        <Search size={14} class="shrink-0" />
        <input
          type="text"
          placeholder="Search agent, run id, user or host"
          bind:value={reportQuery}
          spellcheck="false"
          autocomplete="off"
          aria-label="Search reports"
        />
        {#if reportQuery}
          <button type="button" class="btn-icon btn-icon-sm -mr-1" onclick={() => (reportQuery = "")} aria-label="Clear search">
            <X size={13} />
          </button>
        {/if}
      </label>
      <div class="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter reports">
        {#each REPORT_FILTERS as chip (chip.id)}
          <button
            type="button"
            class="chip {reportFilter === chip.id ? 'chip-active' : ''}"
            aria-pressed={reportFilter === chip.id}
            onclick={() => (reportFilter = chip.id)}
          >
            {chip.label}
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <div class="view-body">
    {#if mode === "reports"}
      {#if reportsLoading && reports.length === 0}
        <div class="grid items-start gap-4 lg:grid-cols-[minmax(260px,340px)_minmax(0,1fr)]" aria-busy="true" aria-label="Loading runs">
          <div class="flex flex-col gap-2">
            {#each [0, 1, 2] as i (i)}
              <div class="card flex flex-col gap-2">
                <div class="skeleton h-3.5 w-32"></div>
                <div class="skeleton h-3 w-48"></div>
              </div>
            {/each}
          </div>
          <div class="panel flex flex-col gap-2.5 p-4">
            <div class="skeleton h-4 w-40"></div>
            <div class="skeleton h-3 w-3/4"></div>
            <div class="skeleton h-16 w-full"></div>
          </div>
        </div>
      {:else if reportsError && reports.length === 0}
        <div class="empty-state h-full">
          <div class="empty-state-icon"><FileText size={18} /></div>
          <div class="empty-state-title">Could not load the reports</div>
          <p class="empty-state-desc" role="alert">{reportsError}</p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={refreshReports}>
              <RefreshCw size={14} />
              Try again
            </button>
          </div>
        </div>
      {:else if reports.length === 0}
        <div class="empty-state h-full">
          <div class="empty-state-icon"><FileText size={18} /></div>
          <div class="empty-state-title">No agent runs recorded yet</div>
          <p class="empty-state-desc">
            A report is written for every run of an AI agent that works through bssh, built from the signed
            audit log. Set an agent up with <span class="mono">bssh ai install claude-code</span> (or codex,
            cursor, vscode, claude-desktop, omp), restart it, and its runs show up here with everything it
            did, step by step, ready to download or print.
          </p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={() => setMode("live")}>
              <Bot size={14} />
              See live runs
            </button>
          </div>
        </div>
      {:else}
        <div class="grid items-start gap-4 lg:grid-cols-[minmax(260px,340px)_minmax(0,1fr)]">
          <ul class="m-0 flex list-none flex-col gap-2 p-0" aria-label="Past runs">
            {#if shownReports.length === 0}
              <li class="type-meta px-1 py-2">
                No run matches.
                <button
                  type="button"
                  class="link"
                  onclick={() => {
                    reportQuery = "";
                    reportFilter = "all";
                  }}>Clear the filters</button
                >
              </li>
            {/if}
            {#each shownReports as item (item.run_id)}
              {@const isSelected = reportRunId === item.run_id}
              <li>
                <button
                  type="button"
                  class="card card-interactive w-full text-left {isSelected ? 'card-selected' : ''}"
                  aria-pressed={isSelected}
                  onclick={() => (reportId = item.run_id)}
                >
                  <span class="flex items-center justify-between gap-2">
                    <span class="truncate text-sm font-medium text-primary" title={item.client ?? ""}>{item.client || "Unknown agent"}</span>
                    {#if item.ended}
                      <span class="badge badge-neutral" title={item.end_reason ?? item.ended}>Ended</span>
                    {:else if liveIds.has(item.run_id)}
                      <span class="badge badge-accent" title="This run is connected now">Running</span>
                    {:else}
                      <span class="badge badge-neutral" title="The audit log has no end for this run">No end recorded</span>
                    {/if}
                  </span>
                  <span class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted">
                    {#if item.started}
                      <span title={formatDateTime(item.started, timezone)}>started {formatRelative(item.started, now)}</span>
                    {:else}
                      <span>start not recorded</span>
                    {/if}
                    <span class="tabular-nums">{item.calls} {item.calls === 1 ? "call" : "calls"}</span>
                    {#if item.denials > 0}
                      <span class="tabular-nums text-error">{item.denials} denied</span>
                    {/if}
                    {#if item.changes > 0}
                      <span class="tabular-nums">{item.changes} {item.changes === 1 ? "change" : "changes"}</span>
                    {/if}
                    {#if (item.approvals ?? 0) > 0}
                      <span class="tabular-nums">{item.approvals} {item.approvals === 1 ? "approval" : "approvals"}</span>
                    {/if}
                  </span>
                  {#if item.on_behalf_of || (item.hosts && item.hosts.length > 0)}
                    <span class="mt-1 block truncate text-xs text-muted">
                      {#if item.on_behalf_of}for {item.on_behalf_of}{/if}{#if item.on_behalf_of && item.hosts?.length}{" · "}{/if}{#if item.hosts?.length}{item.hosts.join(", ")}{/if}
                    </span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>

          <div class="flex min-w-0 flex-col gap-4">
            {#if reportsError}
              <div class="alert alert-error" role="alert">
                <div class="alert-title">Could not refresh the list</div>
                <p class="m-0 text-xs">{reportsError}</p>
              </div>
            {/if}
            {#if savedFile}
              <div class="alert alert-success flex flex-wrap items-center gap-x-3 gap-y-2" role="status">
                <div class="min-w-0 flex-1">
                  <div class="alert-title">Report saved</div>
                  <p class="m-0 break-all text-xs">Report saved to {savedFile.path}</p>
                </div>
                <div class="flex items-center gap-2">
                  <button type="button" class="btn btn-secondary btn-sm" onclick={() => filePath("ai_open_path")}>
                    <ExternalLink size={13} />
                    Open
                  </button>
                  <button type="button" class="btn btn-secondary btn-sm" onclick={() => filePath("ai_reveal_path")}>
                    <FolderOpen size={13} />
                    Show in folder
                  </button>
                  <button type="button" class="btn-icon btn-icon-sm" onclick={() => (savedFile = null)} aria-label="Dismiss">
                    <X size={13} />
                  </button>
                </div>
              </div>
            {/if}
            {#if shownReport}
              <RunReport report={shownReport} {timezone} actions={reportActions} />
            {:else if reportLoading}
              <div class="panel flex flex-col gap-2.5 p-4" aria-busy="true" aria-label="Loading report">
                <div class="skeleton h-4 w-40"></div>
                <div class="skeleton h-3 w-3/4"></div>
                <div class="skeleton h-16 w-full"></div>
              </div>
            {:else if reportError}
              <div class="alert alert-error" role="alert">
                <div class="alert-title">Could not load this report</div>
                <p class="m-0 text-xs">{reportError}</p>
              </div>
            {/if}
          </div>
        </div>
      {/if}
    {:else if runs.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Bot size={18} /></div>
        <div class="empty-state-title">No agent is connected</div>
        <p class="empty-state-desc">
          Set one up with <span class="mono">bssh ai install claude-code</span> (or codex, cursor,
          vscode, claude-desktop, omp), then restart it. Runs appear here while it works through
          bssh.
        </p>
        <div class="empty-state-action">
          <button type="button" class="btn btn-secondary" onclick={() => setMode("reports")}>
            <FileText size={14} />
            Browse past reports
          </button>
        </div>
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
              {#if run.ended !== null}
                <button
                  type="button"
                  class="btn btn-ghost btn-sm mt-1"
                  aria-label="Open the report of {run.client || 'this run'}, {shortId(run.run_id)}"
                  onclick={() => openReport(run.run_id)}
                >
                  <FileText size={13} />
                  Open report
                </button>
              {/if}
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
                <div class="flex items-center gap-2">
                  <button
                    type="button"
                    class="btn btn-secondary"
                    title="Open this run's report from the audit log"
                    onclick={() => openReport(selected.run_id)}
                  >
                    <FileText size={14} />
                    Report
                  </button>
                  {#if selected.paused}
                    <button
                      type="button"
                      class="btn btn-primary"
                      disabled={stopping || resuming}
                      title={selected.pause_reason ?? "Resume this paused run"}
                      onclick={() => resume(selected)}
                    >
                      <Play size={14} />
                      Resume
                    </button>
                  {/if}
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

{#if cleanup}
  <CleanupDialog {timezone} initialRunId={cleanup.runId} onClose={() => (cleanup = null)} onDone={onPruned} />
{/if}
