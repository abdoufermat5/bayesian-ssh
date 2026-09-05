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

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden bg-surface select-none">
  <!-- Header Bar -->
  <div class="px-6 py-4 border-b border-border flex items-center justify-between gap-4 shrink-0 bg-surface-input/30 flex-wrap">
    <div class="flex items-center gap-3">
      <div class="w-8 h-8 rounded-xl bg-accent/15 border border-accent/30 flex items-center justify-center text-accent shrink-0">
        <Clock size={18} />
      </div>
      <div>
        <h2 class="text-sm font-bold text-primary tracking-tight m-0 flex items-center gap-2">
          Session History &amp; Audit Logs
          <span class="badge-pill bg-accent/15 text-accent border border-accent/30 text-[10px]">
            {history.length} records
          </span>
        </h2>
        <p class="text-[11px] text-muted m-0">Historical SSH connection logs, exit codes, and timestamps</p>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <!-- Filter input -->
      <div class="relative flex items-center bg-surface-input border border-border rounded-lg px-2.5 py-1 w-48">
        <Search size={12} class="text-muted mr-1.5 shrink-0" />
        <input
          type="text"
          placeholder="Filter logs..."
          bind:value={filterQuery}
          class="bg-transparent border-none text-xs text-primary outline-none w-full placeholder:text-muted"
        />
        {#if filterQuery}
          <button
            type="button"
            onclick={() => (filterQuery = "")}
            class="text-muted hover:text-primary p-0.5 border-none bg-transparent cursor-pointer"
          >
            <X size={11} />
          </button>
        {/if}
      </div>

      {#if history.length > 0}
        <button
          type="button"
          class="btn btn-secondary shadow-sm"
          onclick={exportHistoryCsv}
          title="Export CSV audit log"
        >
          <Download size={13} />
          <span>Export CSV</span>
        </button>
      {/if}
    </div>
  </div>

  <!-- Table Content Area -->
  <div class="flex-1 min-h-0 overflow-y-auto px-6 py-4 scrollbar-none">
    {#if filteredHistory.length > 0}
      <div class="border border-border rounded-xl overflow-hidden bg-surface-input/30 shadow-sm">
        <!-- Table Header -->
        <div class="flex items-center px-4 py-2.5 bg-surface-input/80 border-b border-border text-[10px] font-bold text-muted uppercase tracking-wider">
          <div class="flex-[3]">Host Connection</div>
          <div class="flex-[2.5]">Started At</div>
          <div class="flex-[2.5] hidden sm:block">Ended At</div>
          <div class="flex-[1.5]">Duration</div>
          <div class="flex-[2]">Status</div>
          <div class="w-16 text-right">Exit Code</div>
        </div>

        <!-- Table Rows -->
        <div class="divide-y divide-border/60">
          {#each filteredHistory as entry}
            {@const success = isSuccessEntry(entry)}
            <div class="flex items-center px-4 py-2.5 text-xs text-secondary hover:bg-white/[0.04] hover:text-primary transition-colors">
              <!-- Connection Name -->
              <div class="flex-[3] flex items-center gap-2 font-semibold text-primary truncate">
                <Terminal size={13} class="text-accent shrink-0" />
                <span class="truncate">{entry.connection_name}</span>
              </div>

              <!-- Started At -->
              <div class="flex-[2.5] font-mono text-[11px] text-muted truncate">
                {formatDateTime(entry.started_at, timezone)}
              </div>

              <!-- Ended At -->
              <div class="flex-[2.5] hidden sm:block font-mono text-[11px] text-muted truncate">
                {entry.ended_at ? formatDateTime(entry.ended_at, timezone) : "Active/Stale"}
              </div>

              <!-- Duration -->
              <div class="flex-[1.5] font-mono text-[11px] text-muted">
                {formatDuration(entry.duration)}
              </div>

              <!-- Status -->
              <div class="flex-[2] flex items-center gap-1.5 truncate">
                {#if success}
                  <span class="badge-pill bg-running/15 text-running border border-running/30">
                    <CheckCircle2 size={11} />
                    <span>{statusLabel(entry.status)}</span>
                  </span>
                {:else}
                  <span class="badge-pill bg-error/15 text-error border border-error/30">
                    <AlertCircle size={11} />
                    <span>{statusLabel(entry.status)}</span>
                  </span>
                {/if}
              </div>

              <!-- Exit Code -->
              <div class="w-16 text-right font-mono text-[11px] text-muted">
                {entry.exit_code !== undefined ? entry.exit_code : "—"}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {:else}
      <div class="py-20 flex flex-col items-center justify-center text-muted border border-dashed border-border rounded-2xl bg-surface-input/10">
        <div class="w-12 h-12 rounded-2xl bg-accent/10 border border-accent/25 flex items-center justify-center text-accent mb-3">
          <Clock size={24} />
        </div>
        <h3 class="text-sm font-bold text-primary mb-1">No Historical Session Logs</h3>
        <p class="text-xs text-muted max-w-sm text-center leading-relaxed">
          {filterQuery ? "No logs match your filter." : "Session records will automatically appear here whenever you establish an SSH session."}
        </p>
      </div>
    {/if}
  </div>
</div>
