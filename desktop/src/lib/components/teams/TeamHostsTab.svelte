<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Share2, Trash2 } from "lucide-svelte";
  import type { Connection, TeamSummary, TeamSyncReport } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import UnshareModal from "./UnshareModal.svelte";

  let {
    team,
    teamConnections,
    localConnections,
    onSynced,
  }: {
    team: TeamSummary;
    teamConnections: Connection[];
    localConnections: Connection[];
    /** A share/unshare ran; the parent applies the report and reloads hosts. */
    onSynced: (report: TeamSyncReport) => void;
  } = $props();

  let choice = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let unsharing = $state.raw<Connection | null>(null);

  async function share() {
    if (!choice || busy) return;
    busy = true;
    error = null;
    try {
      const report = await invoke<TeamSyncReport>("teams_share", { connection: choice, team: team.team_id });
      notify(`Shared '${choice}' with ${team.name}`, "success");
      choice = "";
      onSynced(report);
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-5">
  <section class="flex flex-col gap-2" aria-labelledby="team-hosts-heading">
    <h3 id="team-hosts-heading" class="m-0 text-sm font-semibold text-primary">
      Team hosts <span class="count ml-1">{teamConnections.length}</span>
    </h3>
    {#if teamConnections.length}
      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr><th>Name</th><th>Address</th><th><span class="sr-only">Actions</span></th></tr>
          </thead>
          <tbody>
            {#each teamConnections as c (c.id)}
              <tr>
                <td class="text-primary">{c.name}</td>
                <td class="font-mono text-xs">{c.user}@{c.host}:{c.port}</td>
                <td class="text-right">
                  <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    onclick={() => (unsharing = c)}
                    aria-label="Unshare {c.name}"
                  >
                    <Trash2 size={14} />
                    Unshare
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <p class="m-0 text-xs text-muted">None yet. Sync to fetch them, or share one below.</p>
    {/if}
  </section>

  <section class="flex flex-col gap-2" aria-labelledby="team-share-heading">
    <h3 id="team-share-heading" class="m-0 flex items-center gap-1.5 text-sm font-semibold text-primary">
      <Share2 size={14} /> Share a host with {team.name}
    </h3>
    <form
      class="flex flex-wrap items-end gap-2"
      onsubmit={(e) => {
        e.preventDefault();
        void share();
      }}
    >
      <div class="field">
        <label class="field-label" for="team-share-choice">Your host</label>
        <select id="team-share-choice" class="input w-72" bind:value={choice}>
          <option value="">Choose a host…</option>
          {#each localConnections as c (c.id)}
            <option value={c.name}>{c.name} ({c.user}@{c.host})</option>
          {/each}
        </select>
      </div>
      <button type="submit" class="btn btn-secondary" disabled={busy || !choice}>
        {#if busy}<span class="spinner size-3"></span>{/if}
        Share
      </button>
    </form>
    <p class="m-0 text-xs text-muted">
      Host, user, port, bastion, Kerberos and tags are shared; key paths and passwords never are. Changes sync to
      everyone in {team.name}.
    </p>
    {#if error}<div class="alert alert-error" role="alert">Couldn't share: {error}</div>{/if}
  </section>
</div>

{#if unsharing}
  <UnshareModal
    {team}
    connection={unsharing}
    onClose={() => (unsharing = null)}
    onRemoved={(report) => {
      notify(`Removed '${unsharing?.name}' from ${team.name} for everyone`, "success");
      unsharing = null;
      onSynced(report);
    }}
  />
{/if}
