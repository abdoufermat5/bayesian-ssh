<script lang="ts">
  import { CheckCircle2, AlertCircle } from "lucide-svelte";
  import type { SessionHistoryEntry } from "$lib/types";

  interface Props {
    history: SessionHistoryEntry[];
  }

  let { history }: Props = $props();
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
              <td class="font-semibold text-white">{entry.name}</td>
              <td>{new Date(entry.started_at).toLocaleString()}</td>
              <td>{entry.ended_at ? new Date(entry.ended_at).toLocaleString() : "Active/Stale"}</td>
              <td>
                {#if entry.status.includes("Connected") || entry.status.includes("Completed") || entry.status.includes("Active")}
                  <span class="status-badge success">
                    <CheckCircle2 size={12} /> Active
                  </span>
                {:else}
                  <span class="status-badge error">
                    <AlertCircle size={12} /> Failed
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
