<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Share2, Terminal, Trash2 } from "lucide-svelte";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import type { Connection, TeamSummary, TeamSyncReport } from "$lib/types";
  import { appState } from "$lib/stores/appState.svelte";
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

  const localHostOptions = $derived(
    localConnections.map((c) => ({
      value: c.name,
      label: `${c.name} (${c.user}@${c.host})`,
    })),
  );

  function resolveConnection(c: Connection): Connection {
    return appState.allConnections.find((conn) => conn.id === c.id || conn.name === c.name) ?? c;
  }

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
    <h3 id="team-hosts-heading" class="section-heading">
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
                  <div class="inline-flex items-center justify-end gap-1">
                    <button
                      type="button"
                      class="btn btn-secondary btn-sm"
                      onclick={() => void appState.handleConnect(resolveConnection(c))}
                      aria-label="Connect to {c.name}"
                    >
                      <Terminal size={14} />
                      Connect
                    </button>
                    <button
                      type="button"
                      class="btn btn-ghost btn-sm"
                      onclick={() => (unsharing = c)}
                      aria-label="Unshare {c.name}"
                    >
                      <Trash2 size={14} />
                      Unshare
                    </button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <p class="m-0 type-meta">None yet. Sync to fetch them, or share one below.</p>
    {/if}
  </section>

  <section class="flex flex-col gap-2" aria-labelledby="team-share-heading">
    <h3 id="team-share-heading" class="section-heading flex items-center gap-1.5">
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
        <CustomSelect
          id="team-share-choice"
          class="w-72"
          options={localHostOptions}
          bind:value={choice}
          placeholder="Choose a host…"
          disabled={localHostOptions.length === 0}
        />
      </div>
      <button type="submit" class="btn btn-secondary" disabled={busy || !choice}>
        {#if busy}<span class="spinner size-3"></span>{/if}
        Share
      </button>
    </form>
    <p class="m-0 type-meta">
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
