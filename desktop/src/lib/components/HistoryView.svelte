<script lang="ts">
  import { CheckCircle2, AlertCircle } from "lucide-svelte";
  import type { SessionHistoryEntry } from "$lib/types";
  import { formatDateTime } from "$lib/utils/timezone";

  interface Props {
    history: SessionHistoryEntry[];
    timezone: string;
  }

  let { history, timezone }: Props = $props();

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
</script>

<div class="page-view history-view">
  <div class="page-view-header view-header">
    <div class="title-meta">
      <h2>Session Logs</h2>
      <span class="subtitle">Complete audit logs of connection sessions</span>
    </div>
  </div>

  <div class="page-view-scroll">
    <div class="table-container">
      <table class="history-table">
        <thead>
          <tr>
            <th>Host Connection</th>
            <th>Started At</th>
            <th>Ended At</th>
            <th>Status</th>
            <th>Exit Code</th>
          </tr>
        </thead>
        <tbody>
          {#each history as entry}
            <tr>
              <td class="font-semibold text-white">{entry.connection_name}</td>
              <td>{formatDateTime(entry.started_at, timezone)}</td>
              <td>{entry.ended_at ? formatDateTime(entry.ended_at, timezone) : "Active/Stale"}</td>
              <td>
                {#if isSuccessEntry(entry)}
                  <span class="status-badge success">
                    <CheckCircle2 size={12} /> {statusLabel(entry.status)}
                  </span>
                {:else}
                  <span class="status-badge error">
                    <AlertCircle size={12} /> {statusLabel(entry.status)}
                  </span>
                {/if}
              </td>
              <td class="font-mono">{entry.exit_code !== undefined ? entry.exit_code : "-"}</td>
            </tr>
          {:else}
            <tr>
              <td colspan="5" style="text-align: center; padding: 3rem; color: var(--text-muted);">
                No historical logs found.
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</div>
