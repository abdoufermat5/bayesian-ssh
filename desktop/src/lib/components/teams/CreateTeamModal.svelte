<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { TeamSyncReport } from "$lib/types";

  let {
    onClose,
    onCreated,
  }: { onClose: () => void; onCreated: (teamId: string, name: string) => void } = $props();

  let name = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function create() {
    const trimmed = name.trim();
    if (!trimmed || busy) return;
    busy = true;
    error = null;
    try {
      const report = await invoke<TeamSyncReport>("teams_create", { name: trimmed });
      onCreated(report.team_id, trimmed);
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<ModalShell open={true} title="Create a team" {onClose} width="md">
  <form
    onsubmit={(e) => {
      e.preventDefault();
      void create();
    }}
  >
    <div class="modal-header">
      <h2 class="modal-title">Create a team</h2>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close"><X size={16} /></button>
    </div>
    <div class="modal-body flex flex-col gap-3">
      <div class="field">
        <label class="field-label" for="teams-new-name">Team name</label>
        <input id="teams-new-name" class="input" bind:value={name} placeholder="platform" />
        <span class="field-hint">You'll be its admin. Its hosts show up as <code>name@team</code> in your host list.</span>
      </div>
      {#if error}<div class="alert alert-error" role="alert">Couldn't create the team: {error}</div>{/if}
      <p class="m-0 text-xs text-muted">CLI: <code class="select-text">bssh teams create {name.trim() || "NAME"}</code></p>
    </div>
    <div class="modal-footer">
      {#if !name.trim()}<span class="mr-auto text-xs text-muted">Enter a team name</span>{/if}
      <button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
      <button type="submit" class="btn btn-primary" disabled={busy || !name.trim()}>
        {#if busy}<span class="spinner size-3"></span>{/if}
        Create team
      </button>
    </div>
  </form>
</ModalShell>
