<script lang="ts">
  import {
    AlertCircle,
    CheckCircle2,
    Clock,
    Download,
    Search,
    Terminal,
    X,
  } from "lucide-svelte";
  import type { SessionHistoryEntry } from "$lib/types";
  import { formatDateTime } from "$lib/utils/timezone";

  interface Props {
    history: SessionHistoryEntry[];
    timezone: string;
  }

  let { history, timezone }: Props = $props();

  let filterQuery = $state("");

  function statusLabel(status: SessionHistoryEntry["status"]): string {
    if (typeof status === "string") return status;
    if (status && typeof status === "object" && "Error" in status) {
      return `Error: ${status.Error}`;
    }
    return "Unknown";
  }

  function isSuccessEntry(entry: SessionHistoryEntry): boolean {
    if (typeof entry.status === "object" && entry.status !== null && "Error" in entry.status) {
      return false;
    }

    if (typeof entry.status === "string") {
      if (entry.status === "Active" || entry.status === "Starting") return true;
      if (entry.status === "Disconnected") return true;
      if (entry.status === "Terminated") {
        return entry.exit_code === undefined || entry.exit_code === 0;
      }
    }

    return false;
  }

  function formatDuration(seconds?: number): string {
    if (!seconds || seconds <= 0) return "—";
    if (seconds < 60) return `${Math.round(seconds)}s`;
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    if (m < 60) return `${m}m ${s}s`;
    const h = Math.floor(m / 60);
    return `${h}h ${m % 60}m`;
  }

  const filteredHistory = $derived.by(() => {
    if (!filterQuery.trim()) return history;
    const q = filterQuery.toLowerCase();
    return history.filter(
      (h) =>
        h.connection_name.toLowerCase().includes(q) ||
        statusLabel(h.status).toLowerCase().includes(q),
    );
  });

  function exportHistoryCsv() {
    const lines = ["Connection,StartedAt,EndedAt,Status,ExitCode,DurationSec"];
    for (const h of history) {
      lines.push(
        `"${h.connection_name}","${h.started_at}","${h.ended_at ?? ""}","${statusLabel(h.status)}","${h.exit_code ?? ""}","${h.duration ?? ""}"`,
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

<div class="flex min-h-0 w-full flex-1 flex-col overflow-hidden bg-surface text-[13px] text-primary select-none">
  <div class="view-header">
    <div class="flex min-w-0 items-center gap-3">
      <div class="icon-tile rounded-md">
        <Clock size={16} />
      </div>
      <div class="min-w-0">
        <h2 class="m-0 flex items-center gap-2 truncate text-sm font-bold tracking-tight text-primary">
          History
          <span class="badge badge-subtle">{history.length} records</span>
        </h2>
        <p class="m-0 truncate text-xs text-muted">SSH sessions, timing, status, and exit codes</p>
      </div>
    </div>

    <div class="flex min-w-[260px] flex-1 items-center justify-end gap-2 sm:flex-none">
      <div class="search-box w-full sm:w-56">
        <Search size={13} class="shrink-0 text-muted" />
        <input
          type="text"
          placeholder="Filter logs..."
          bind:value={filterQuery}
          class="w-full border-none bg-transparent text-xs text-primary outline-none placeholder:text-muted"
        />
        {#if filterQuery}
          <button
            type="button"
            onclick={() => (filterQuery = "")}
            class="btn-icon p-0.5"
            title="Clear filter"
          >
            <X size={11} />
          </button>
        {/if}
      </div>

      {#if history.length > 0}
        <button
          type="button"
          class="btn btn-secondary"
          onclick={exportHistoryCsv}
          title="Export CSV audit log"
        >
          <Download size={13} />
          <span>Export CSV</span>
        </button>
      {/if}
    </div>
  </div>

  <div class="view-content">
    {#if filteredHistory.length > 0}
      <div class="data-table overflow-x-auto">
        <table class="w-full border-collapse">
          <thead>
            <tr class="border-b border-border bg-surface-input/80 text-left table-header">
              <th class="px-4 py-2.5 font-bold">Connection</th>
              <th class="hidden sm:table-cell px-4 py-2.5 font-bold">Started</th>
              <th class="hidden md:table-cell px-4 py-2.5 font-bold">Ended</th>
              <th class="px-4 py-2.5 font-bold">Duration</th>
              <th class="px-4 py-2.5 font-bold">Status</th>
              <th class="px-4 py-2.5 text-right font-bold">Exit</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-border/60">
          {#each filteredHistory as entry}
            {@const success = isSuccessEntry(entry)}
            <tr class="row border-0 text-xs text-secondary">
              <td class="max-w-[220px] px-4 py-2.5">
                <div class="flex min-w-0 items-center gap-2 font-semibold text-primary">
                  <Terminal size={13} class="shrink-0 text-accent" />
                  <span class="truncate" title={entry.connection_name}>{entry.connection_name}</span>
                </div>
              </td>
              <td class="hidden sm:table-cell px-4 py-2.5 font-mono text-xs text-muted">{formatDateTime(entry.started_at, timezone)}</td>
              <td class="hidden md:table-cell px-4 py-2.5 font-mono text-xs text-muted">
                {entry.ended_at ? formatDateTime(entry.ended_at, timezone) : "Active"}
              </td>
              <td class="px-4 py-2.5 font-mono text-xs text-muted">{formatDuration(entry.duration)}</td>
              <td class="px-4 py-2.5">
                {#if success}
                  <span class="badge badge-success">
                    <CheckCircle2 size={11} />
                    <span>{statusLabel(entry.status)}</span>
                  </span>
                {:else}
                  <span class="badge badge-error">
                    <AlertCircle size={11} />
                    <span>{statusLabel(entry.status)}</span>
                  </span>
                {/if}
              </td>
              <td class="px-4 py-2.5 text-right font-mono text-xs text-muted">
                {entry.exit_code !== undefined ? entry.exit_code : "-"}
              </td>
            </tr>
          {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="empty-state empty-state-dashed">
        <div class="empty-state-icon">
          <Clock size={22} />
        </div>
        <h3 class="empty-state-title">No session history</h3>
        <p class="empty-state-desc">
          {filterQuery ? "No logs match your filter." : "Session records will automatically appear here whenever you establish an SSH session."}
        </p>
      </div>
    {/if}
  </div>
</div>
