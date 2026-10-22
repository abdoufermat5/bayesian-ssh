<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { Connection, TeamSummary, TeamSyncReport } from "$lib/types";

  let {
    team,
    connection,
    onClose,
    onRemoved,
  }: {
    team: TeamSummary;
    connection: Connection;
    onClose: () => void;
    onRemoved: (report: TeamSyncReport) => void;
  } = $props();

  let busy = $state(false);
  let error = $state<string | null>(null);

  async function unshare() {
    busy = true;
    error = null;
    try {
      onRemoved(await invoke<TeamSyncReport>("teams_unshare", { connection: connection.name }));
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<ModalShell open={true} title="Remove {connection.name} for everyone in {team.name}?" {onClose} width="sm">
  <div class="modal-header">
    <h2 class="modal-title break-words">Remove {connection.name} for everyone in {team.name}?</h2>
    <button type="button" class="modal-close" onclick={onClose} aria-label="Close"><X size={16} /></button>
  </div>
  <div class="modal-body flex flex-col gap-2.5 text-sm text-secondary">
    <div class="code-block whitespace-pre-wrap break-all">{connection.user}@{connection.host}:{connection.port}</div>
    <p class="m-0 leading-relaxed">
      Every member of {team.name} loses this host at their next sync. Older versions stay in the server's history.
    </p>
    {#if error}<div class="alert alert-error" role="alert">Couldn't remove it: {error}</div>{/if}
    <p class="m-0 text-xs text-muted">CLI: <code class="select-text">bssh teams unshare {connection.name}</code></p>
  </div>
  <div class="modal-footer">
    <button type="button" class="btn btn-secondary" onclick={onClose} data-autofocus>Cancel</button>
    <button type="button" class="btn btn-danger" onclick={unshare} disabled={busy}>
      {#if busy}<span class="spinner size-3"></span>{/if}
      Remove for everyone
    </button>
  </div>
</ModalShell>
