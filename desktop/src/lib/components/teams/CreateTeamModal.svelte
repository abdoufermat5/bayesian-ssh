<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { OrgSummary, TeamSyncReport } from "$lib/types";

  let {
    orgs = null,
    onClose,
    onCreated,
  }: {
    /** Organizations of this account; null on a server without them. */
    orgs?: OrgSummary[] | null;
    onClose: () => void;
    onCreated: (teamId: string, name: string) => void;
  } = $props();

  /** Owned organizations, oldest first: the server puts a team in the oldest one by default. */
  const owned = $derived(
    (orgs ?? []).filter((o) => o.my_role === "owner").sort((a, b) => a.created_at.localeCompare(b.created_at)),
  );
  let chosen = $state<string | null>(null);
  const orgId = $derived(chosen ?? owned[0]?.org_id ?? null);

  let name = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function create() {
    const trimmed = name.trim();
    if (!trimmed || busy) return;
    busy = true;
    error = null;
    try {
      const report = await invoke<TeamSyncReport>("teams_create", { name: trimmed, org: owned.length > 1 ? orgId : null });
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
      {#if owned.length > 1}
        <div class="field">
          <label class="field-label" for="teams-new-org">Organization</label>
          <select id="teams-new-org" class="input" value={orgId} onchange={(e) => (chosen = e.currentTarget.value)}>
            {#each owned as o (o.org_id)}
              <option value={o.org_id}>{o.label} ({o.short_id})</option>
            {/each}
          </select>
          <span class="field-hint">Its seats and billing are shared with the organization's other teams.</span>
        </div>
      {/if}
      {#if error}<div class="alert alert-error" role="alert">Couldn't create the team: {error}</div>{/if}
      <p class="m-0 type-meta">CLI: <code class="select-text">bssh teams create {name.trim() || "NAME"}{owned.length > 1 && orgId ? ` --org ${owned.find((o) => o.org_id === orgId)?.short_id}` : ""}</code></p>
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
