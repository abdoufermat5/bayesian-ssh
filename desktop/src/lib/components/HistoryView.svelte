<script lang="ts">
  import { CheckCircle2, AlertCircle, Clock } from "lucide-svelte";
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

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden">
  <div class="shrink-0 px-6 pt-5 pb-3">
    <h2 class="text-lg m-0 font-semibold tracking-tight text-primary">Session Logs</h2>
    <span class="text-muted text-xs mt-0.5 block">Complete audit logs of connection sessions</span>
  </div>

  <div class="flex-1 min-h-0 overflow-y-auto overflow-x-hidden px-6 pb-6 overscroll-contain">
    <div class="border border-border rounded-xl overflow-hidden">
      <table class="w-full border-collapse text-[13px] text-secondary">
        <thead>
          <tr class="bg-white/[0.02] border-b border-border text-[11px] font-semibold text-muted uppercase tracking-wider">
            <th class="text-left px-4 py-2.5">Host Connection</th>
            <th class="text-left px-4 py-2.5">Started At</th>
            <th class="text-left px-4 py-2.5">Ended At</th>
            <th class="text-left px-4 py-2.5">Status</th>
            <th class="text-left px-4 py-2.5">Exit Code</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-border">
          {#each history as entry}
            <tr class="transition-colors duration-100 hover:bg-white/[0.02]">
              <td class="px-4 py-2.5 font-semibold text-primary">{entry.connection_name}</td>
              <td class="px-4 py-2.5">{formatDateTime(entry.started_at, timezone)}</td>
              <td class="px-4 py-2.5">{entry.ended_at ? formatDateTime(entry.ended_at, timezone) : "Active/Stale"}</td>
              <td class="px-4 py-2.5">
                {#if isSuccessEntry(entry)}
                  <span class="inline-flex items-center gap-1 font-semibold text-success">
                    <CheckCircle2 size={12} /> {statusLabel(entry.status)}
                  </span>
                {:else}
                  <span class="inline-flex items-center gap-1 font-semibold text-danger">
                    <AlertCircle size={12} /> {statusLabel(entry.status)}
                  </span>
                {/if}
              </td>
              <td class="px-4 py-2.5 font-mono">{entry.exit_code !== undefined ? entry.exit_code : "-"}</td>
            </tr>
          {:else}
            <tr>
              <td colspan="5">
                <div class="flex flex-col items-center justify-center px-6 py-14 text-center">
                  <Clock size={40} class="text-muted opacity-80 mb-3" />
                  <span class="text-sm font-semibold text-primary mb-1">No historical logs found.</span>
                  <span class="text-xs text-muted m-0 max-w-[320px] leading-relaxed">
                    Session history will appear here after you connect to a host.
                  </span>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</div>
