<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Clock, Download, FileText, Play, Search, X } from "lucide-svelte";
  import ReplayModal from "$lib/components/modals/ReplayModal.svelte";
  import type { SessionHistoryEntry, SessionTextHit } from "$lib/types";
  import { formatDate, formatDateTime, formatRelative, resolveTimeZone } from "$lib/utils/timezone";

  interface Props {
    history: SessionHistoryEntry[];
    timezone: string;
  }

  let { history, timezone }: Props = $props();

  type Outcome = "success" | "failed" | "running";
  type Filter = "all" | "success" | "failed";

  interface Row {
    key: string;
    sessionId: string;
    recorded: boolean;
    name: string;
    search: string;
    startedAt: string;
    startedMs: number;
    relative: string;
    absolute: string;
    dayKey: string;
    durationSec: number | null;
    outcome: Outcome;
    /** Raw backend status variant, e.g. "Terminated" or "Error". */
    detail: string;
    /** Message carried by `{ Error: string }`, if any. */
    error: string | null;
    exitCode: number | undefined;
  }

  interface SnippetPart {
    text: string;
    highlight: boolean;
  }

  const OUTCOME: Record<Outcome, { label: string; badge: string }> = {
    success: { label: "Succeeded", badge: "badge-success" },
    failed: { label: "Failed", badge: "badge-error" },
    running: { label: "Running", badge: "badge-neutral" },
  };

  let filterQuery = $state("");
  let statusFilter = $state<Filter>("all");

  let outputQuery = $state("");
  let outputHits = $state.raw<SessionTextHit[]>([]);
  let outputSearching = $state(false);
  let outputError = $state<string | null>(null);

  let replayTarget = $state<{
    sessionId: string;
    connectionName: string;
    startedLabel: string;
  } | null>(null);

  function openReplay(sessionId: string, connectionName: string, startedLabel: string) {
    if (!sessionId) return;
    replayTarget = { sessionId, connectionName, startedLabel };
  }

  /**
   * Split an FTS5 `snippet(session_text, 1, '[', ']', '…', 12)` string into
   * plain and highlighted runs so `[`…`]` markers render as `<mark>` elements
   * without `{@html}` on untrusted terminal output.
   */
  function parseSnippetParts(snippet: string): SnippetPart[] {
    const parts: SnippetPart[] = [];
    let rest = snippet;
    while (rest.length > 0) {
      const openIdx = rest.indexOf("[");
      if (openIdx === -1) {
        parts.push({ text: rest, highlight: false });
        break;
      }
      const closeIdx = rest.indexOf("]", openIdx + 1);
      if (closeIdx === -1) {
        parts.push({ text: rest, highlight: false });
        break;
      }
      if (openIdx > 0) {
        parts.push({ text: rest.slice(0, openIdx), highlight: false });
      }
      parts.push({ text: rest.slice(openIdx + 1, closeIdx), highlight: true });
      rest = rest.slice(closeIdx + 1);
    }
    return parts;
  }

  $effect(() => {
    const q = outputQuery.trim();
    if (!q) {
      outputHits = [];
      outputSearching = false;
      outputError = null;
      return;
    }

    let cancelled = false;
    outputSearching = true;
    outputError = null;

    const timer = setTimeout(() => {
      invoke<SessionTextHit[]>("search_session_text", { query: q, limit: 50 })
        .then((hits) => {
          if (!cancelled) {
            outputHits = hits;
          }
        })
        .catch((err) => {
          if (!cancelled) {
            outputError = String(err);
            outputHits = [];
          }
        })
        .finally(() => {
          if (!cancelled) {
            outputSearching = false;
          }
        });
    }, 250);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  });

  /**
   * `status` is the serde form of the Rust `SessionStatus` enum: a bare
   * variant name ("Starting" | "Active" | "Disconnected" | "Terminated") or
   * `{ Error: string }`. A finished session only failed if it errored or
   * exited non-zero.
   */
  function outcomeOf(entry: SessionHistoryEntry): Outcome {
    const { status } = entry;
    if (typeof status === "object" && status !== null) return "failed";
    if ((status === "Active" || status === "Starting") && !entry.ended_at) return "running";
    if (entry.exit_code !== undefined && entry.exit_code !== null && entry.exit_code !== 0) return "failed";
    return "success";
  }

  function statusDetail(status: SessionHistoryEntry["status"]): string {
    if (typeof status === "string") return status;
    if (status && typeof status === "object" && "Error" in status) return status.Error;
    return "Unknown";
  }

  /**
   * chrono serialises `Duration` as `[secs, nanos]`; tolerate a plain number
   * too, and fall back to the timestamps.
   */
  function durationSeconds(entry: SessionHistoryEntry): number | null {
    const raw = entry.duration;
    if (typeof raw === "number" && Number.isFinite(raw)) return raw;
    if (Array.isArray(raw) && typeof raw[0] === "number") return raw[0] + (Number(raw[1]) || 0) / 1e9;
    if (entry.ended_at) {
      const ms = Date.parse(entry.ended_at) - Date.parse(entry.started_at);
      if (Number.isFinite(ms)) return ms / 1000;
    }
    return null;
  }

  function formatDuration(seconds: number | null): string {
    if (seconds === null || seconds < 0) return "—";
    if (seconds < 1) return "<1s";
    if (seconds < 60) return `${Math.round(seconds)}s`;
    const total = Math.round(seconds);
    const m = Math.floor(total / 60);
    const s = total % 60;
    if (m < 60) return s ? `${m}m ${s}s` : `${m}m`;
    const h = Math.floor(m / 60);
    return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
  }

  // Derived once per history/timezone change; filtering below is a cheap pass.
  const dayFormatter = $derived(
    new Intl.DateTimeFormat("en-CA", {
      timeZone: resolveTimeZone(timezone),
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }),
  );

  // Newest first, so day groups are contiguous regardless of backend order.
  const rows = $derived.by<Row[]>(() => {
    const now = Date.now();
    const out = history.map((entry, i): Row => {
      const detail = statusDetail(entry.status);
      const error = typeof entry.status === "object" && entry.status !== null ? detail : null;
      const startedMs = Date.parse(entry.started_at);
      const outcome = outcomeOf(entry);
      const sessionId = entry.session_id ?? "";
      return {
        key: `${sessionId}|${entry.started_at}|${entry.connection_name}|${i}`,
        sessionId,
        recorded: Boolean(entry.recording_path),
        name: entry.connection_name,
        search: `${entry.connection_name}\n${detail}\n${sessionId}`.toLowerCase(),
        startedAt: entry.started_at,
        startedMs: Number.isNaN(startedMs) ? 0 : startedMs,
        relative: formatRelative(entry.started_at, now),
        absolute: formatDateTime(entry.started_at, timezone),
        dayKey: Number.isNaN(startedMs) ? "" : dayFormatter.format(startedMs),
        durationSec: outcome === "running" ? null : durationSeconds(entry),
        outcome,
        detail: error === null ? detail : "Error",
        error,
        exitCode: entry.exit_code ?? undefined,
      };
    });
    return out.sort((a, b) => b.startedMs - a.startedMs);
  });

  const counts = $derived.by(() => {
    let success = 0;
    let failed = 0;
    for (const r of rows) {
      if (r.outcome === "success") success++;
      else if (r.outcome === "failed") failed++;
    }
    return { all: rows.length, success, failed };
  });

  const filteredRows = $derived.by(() => {
    const q = filterQuery.trim().toLowerCase();
    if (!q && statusFilter === "all") return rows;
    return rows.filter((r) => (statusFilter === "all" || r.outcome === statusFilter) && (!q || r.search.includes(q)));
  });

  // Consecutive rows sharing a calendar day (history arrives newest first).
  const groups = $derived.by(() => {
    const today = dayFormatter.format(new Date());
    const yesterday = dayFormatter.format(new Date(Date.now() - 86_400_000));
    const out: { key: string; label: string; rows: Row[] }[] = [];
    for (const row of filteredRows) {
      const last = out[out.length - 1];
      if (last && last.key === row.dayKey) {
        last.rows.push(row);
        continue;
      }
      const label =
        row.dayKey === today
          ? "Today"
          : row.dayKey === yesterday
            ? "Yesterday"
            : row.dayKey
              ? formatDate(row.startedAt, timezone)
              : "Unknown date";
      out.push({ key: row.dayKey, label, rows: [row] });
    }
    return out;
  });

  const hasFilters = $derived(filterQuery.trim() !== "" || statusFilter !== "all");
  const isOutputSearchActive = $derived(outputQuery.trim() !== "");

  function clearFilters() {
    filterQuery = "";
    statusFilter = "all";
  }

  function csvCell(value: string | number | null | undefined): string {
    return `"${String(value ?? "").replace(/"/g, '""')}"`;
  }

  function exportHistoryCsv() {
    const lines = ["Connection,StartedAt,EndedAt,Status,ExitCode,DurationSec"];
    for (const h of history) {
      const duration = durationSeconds(h);
      lines.push(
        [
          h.connection_name,
          h.started_at,
          h.ended_at,
          statusDetail(h.status),
          h.exit_code,
          duration === null ? "" : Math.round(duration),
        ]
          .map(csvCell)
          .join(","),
      );
    }
    const blob = new Blob([lines.join("\n")], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `bayesian_ssh_history_${Date.now()}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }
</script>

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">History</h1>
      <span class="text-sm tabular-nums text-muted">
        {#if isOutputSearchActive}
          {outputHits.length} {outputHits.length === 1 ? "match" : "matches"}
        {:else if hasFilters}
          {filteredRows.length} of {rows.length}
        {:else}
          {rows.length}
        {/if}
      </span>
    </div>
    <div class="view-actions">
      <button
        type="button"
        class="btn btn-ghost"
        onclick={exportHistoryCsv}
        disabled={history.length === 0}
        title="Download all sessions as CSV"
      >
        <Download size={14} />
        Export CSV
      </button>
    </div>
  </header>

  {#if rows.length > 0 || isOutputSearchActive}
    <div class="view-toolbar flex-wrap gap-2">
      <label class="search-box w-56 lg:w-64">
        <Search size={14} class="shrink-0" />
        <input
          type="text"
          placeholder="Filter by host or status"
          bind:value={filterQuery}
          spellcheck="false"
          autocomplete="off"
          aria-label="Filter history"
        />
        {#if filterQuery}
          <button
            type="button"
            class="btn-icon btn-icon-sm -mr-1"
            onclick={() => (filterQuery = "")}
            aria-label="Clear filter"
          >
            <X size={13} />
          </button>
        {/if}
      </label>

      <label class="search-box w-64 lg:w-80">
        <FileText size={14} class="shrink-0" />
        <input
          type="text"
          placeholder="Search recorded output…"
          bind:value={outputQuery}
          spellcheck="false"
          autocomplete="off"
          aria-label="Search recorded output"
        />
        {#if outputQuery}
          <button
            type="button"
            class="btn-icon btn-icon-sm -mr-1"
            onclick={() => (outputQuery = "")}
            aria-label="Clear recorded output search"
          >
            <X size={13} />
          </button>
        {/if}
      </label>

      {#if !isOutputSearchActive}
        <div class="flex items-center gap-1.5" role="group" aria-label="Filter by status">
          {#each [
            { id: "all", label: "All", count: counts.all },
            { id: "success", label: "Succeeded", count: counts.success },
            { id: "failed", label: "Failed", count: counts.failed },
          ] as chip (chip.id)}
            <button
              type="button"
              class="chip {statusFilter === chip.id ? 'chip-active' : ''}"
              onclick={() => (statusFilter = chip.id as Filter)}
              aria-pressed={statusFilter === chip.id}
            >
              {chip.label}
              <span class="tabular-nums text-muted">{chip.count}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <div class="view-body">
    {#if isOutputSearchActive}
      {#if outputSearching}
        <div class="empty-state h-full">
          <span class="spinner" aria-hidden="true"></span>
          <div class="empty-state-title">Searching recorded output…</div>
        </div>
      {:else if outputError}
        <div class="alert alert-error">
          <div class="alert-title">Search failed</div>
          <p class="m-0 text-xs">{outputError}</p>
        </div>
      {:else if outputHits.length === 0}
        <div class="empty-state h-full">
          <div class="empty-state-icon"><Search size={18} /></div>
          <div class="empty-state-title">No recorded output matches</div>
          <p class="empty-state-desc">No recorded session contained "{outputQuery.trim()}".</p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={() => (outputQuery = "")}>
              Clear search
            </button>
          </div>
        </div>
      {:else}
        <div class="table-wrap">
          <table class="data-table table-fixed">
            <thead>
              <tr>
                <th class="w-[180px]">Host</th>
                <th class="w-[148px]">Started</th>
                <th class="w-[96px]">Session</th>
                <th>Snippet</th>
                <th class="w-[100px] text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each outputHits as hit (hit.session_id)}
                {@const abs = formatDateTime(hit.started_at, timezone)}
                {@const rel = formatRelative(hit.started_at, Date.now())}
                <tr>
                  <td>
                    <span class="block truncate font-medium text-primary" title={hit.connection_name}>
                      {hit.connection_name}
                    </span>
                  </td>
                  <td>
                    <span class="text-xs text-secondary" title={abs}>{rel}</span>
                  </td>
                  <td class="font-mono text-xs text-muted">
                    {hit.session_id.slice(0, 8)}
                  </td>
                  <td class="font-mono text-xs text-secondary">
                    <div class="truncate">
                      {#each parseSnippetParts(hit.snippet) as part, idx (idx)}
                        {#if part.highlight}
                          <mark class="rounded-sm bg-accent-muted px-1 text-primary">{part.text}</mark>
                        {:else}
                          {part.text}
                        {/if}
                      {/each}
                    </div>
                  </td>
                  <td class="text-right">
                    <button
                      type="button"
                      class="btn btn-ghost btn-sm"
                      onclick={() => openReplay(hit.session_id, hit.connection_name, abs)}
                    >
                      <Play size={13} />
                      Replay
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {:else if rows.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Clock size={18} /></div>
        <div class="empty-state-title">No sessions yet</div>
        <p class="empty-state-desc">Every SSH session you open is recorded here with its duration and exit code.</p>
      </div>
    {:else if filteredRows.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Search size={18} /></div>
        <div class="empty-state-title">No sessions match</div>
        <p class="empty-state-desc">Nothing matches the current filter. Try another term or clear it.</p>
        <div class="empty-state-action">
          <button type="button" class="btn btn-secondary" onclick={clearFilters}>Clear filter</button>
        </div>
      </div>
    {:else}
      <div class="table-wrap">
        <table class="data-table table-fixed">
          <thead>
            <tr>
              <th>Host</th>
              <th class="w-[148px]">Started</th>
              <th class="w-[104px] text-right">Duration</th>
              <th class="w-[30%]">Status</th>
              <th class="w-[88px] text-right">Exit code</th>
              <th class="w-[104px] text-right">Recording</th>
            </tr>
          </thead>
          {#each groups as group, gi (group.key)}
            <tbody>
              <tr class="hover:bg-transparent {gi > 0 ? 'border-t border-border' : ''}">
                <td colspan="6" class="h-8 bg-surface/60">
                  <span class="section-label">{group.label}</span>
                  <span class="ml-1.5 text-2xs tabular-nums text-muted">{group.rows.length}</span>
                </td>
              </tr>
              {#each group.rows as row (row.key)}
                {@const outcome = OUTCOME[row.outcome]}
                <tr>
                  <td>
                    <div class="flex min-w-0 items-center gap-2">
                      <span class="block truncate font-medium text-primary" title={row.name}>{row.name}</span>
                      {#if row.recorded}
                        <span
                          class="badge badge-neutral shrink-0"
                          title="Session output recorded ({row.sessionId.slice(0, 8)})"
                        >
                          <span class="status-dot status-dot-sm status-dot-success" title="Recorded"></span>
                          Recorded
                        </span>
                      {/if}
                    </div>
                  </td>
                  <td>
                    <span class="text-xs text-secondary" title={row.absolute}>{row.relative}</span>
                  </td>
                  <td class="text-right font-mono text-xs tabular-nums text-secondary">
                    {formatDuration(row.durationSec)}
                  </td>
                  <td>
                    <div class="flex min-w-0 items-center gap-2">
                      <span
                        class="badge {outcome.badge}"
                        title={row.error ??
                          (row.outcome === "failed" ? `Exited with code ${row.exitCode}` : `Status: ${row.detail}`)}
                      >
                        {#if row.outcome === "running"}
                          <span class="status-dot status-dot-sm status-dot-running status-dot-live"></span>
                        {/if}
                        {outcome.label}
                      </span>
                      {#if row.error}
                        <span class="truncate text-xs text-muted" title={row.error}>{row.error}</span>
                      {/if}
                    </div>
                  </td>
                  <td
                    class="text-right font-mono text-xs tabular-nums {row.exitCode === undefined || row.exitCode === 0
                      ? 'text-muted'
                      : 'text-primary'}"
                  >
                    {row.exitCode ?? "—"}
                  </td>
                  <td class="text-right">
                    {#if row.recorded && row.sessionId}
                      <button
                        type="button"
                        class="btn btn-ghost btn-sm"
                        onclick={() => openReplay(row.sessionId, row.name, row.absolute)}
                        title="Replay recorded session ({row.sessionId.slice(0, 8)})"
                      >
                        <Play size={13} />
                        Replay
                      </button>
                    {:else}
                      <span class="text-xs text-muted">—</span>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          {/each}
        </table>
      </div>
    {/if}
  </div>
</div>

<ReplayModal
  open={replayTarget !== null}
  sessionId={replayTarget?.sessionId ?? null}
  connectionName={replayTarget?.connectionName ?? ""}
  startedLabel={replayTarget?.startedLabel ?? ""}
  onClose={() => (replayTarget = null)}
/>
