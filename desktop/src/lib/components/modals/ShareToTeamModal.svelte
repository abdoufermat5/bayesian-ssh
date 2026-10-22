<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Lock, Share2, Users, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { Connection, TeamAttention, TeamsStatus, TeamSummary, TeamSyncReport } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { TEAMS_CHANGED_EVENT } from "$lib/stores/teamsAttention.svelte";

  interface Props {
    connection: Connection;
    onClose: () => void;
    /** The share ran; the caller reloads its hosts. */
    onShared: () => void;
    onOpenTeams: () => void;
  }

  let { connection, onClose, onShared, onOpenTeams }: Props = $props();

  type Phase = "loading" | "not-set-up" | "locked" | "no-team" | "ready" | "error";

  let phase = $state<Phase>("loading");
  let loadError = $state<string | null>(null);
  let teams = $state.raw<TeamSummary[]>([]);
  /** The team this host already belongs to (a team copy): sharing updates it there. */
  let ownTeam = $state.raw<TeamSummary | null>(null);
  let choice = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  const chosen = $derived(ownTeam ?? teams.find((t) => t.team_id === choice) ?? null);
  const title = $derived(ownTeam ? `Update ${connection.name} in ${ownTeam.name}` : `Share ${connection.name} with a team`);
  const cli = $derived(`bssh teams share ${connection.name} --team ${chosen?.name ?? "<team>"}`);

  $effect(() => {
    void load();
  });

  // The first answer replaces the loading state: move focus to its main control.
  let body = $state<HTMLDivElement>();
  $effect(() => {
    if (phase === "loading" || !body) return;
    const panel = body.closest<HTMLElement>('[role="dialog"]');
    requestAnimationFrame(() => panel?.querySelector<HTMLElement>("[data-autofocus]")?.focus());
  });

  async function load() {
    try {
      const status = await invoke<TeamsStatus | null>("teams_status");
      if (!status?.identity) {
        phase = "not-set-up";
        return;
      }
      if (!status.unlocked) {
        phase = "locked";
        return;
      }
      // Only teams whose key this device holds can take a share.
      let usable = status.teams;
      try {
        const attention = (await invoke<TeamAttention[] | null>("teams_attention")) ?? [];
        const blocked = new Set(attention.filter((a) => a.waiting_for_key || a.removed).map((a) => a.team_id));
        usable = usable.filter((t) => !blocked.has(t.team_id));
      } catch {
        // Offline: keep every local team; the share itself reports a missing key.
      }
      ownTeam = status.teams.find((t) => t.source_id === connection.source_id) ?? null;
      teams = usable;
      if (ownTeam) {
        phase = usable.some((t) => t.team_id === ownTeam?.team_id) ? "ready" : "no-team";
      } else {
        phase = usable.length ? "ready" : "no-team";
        if (usable.length === 1) choice = usable[0].team_id;
      }
    } catch (err) {
      loadError = String(err);
      phase = "error";
    }
  }

  async function share() {
    if (!chosen || busy) return;
    busy = true;
    error = null;
    try {
      const report = await invoke<TeamSyncReport>("teams_share", {
        connection: connection.name,
        team: chosen.team_id,
      });
      notify(
        ownTeam ? `Updated '${connection.name}' in ${chosen.name}` : `Shared '${connection.name}' with ${chosen.name}`,
        "success",
      );
      for (const e of report.errors ?? []) notify(`${chosen.name}: ${e}`, "error");
      window.dispatchEvent(new Event(TEAMS_CHANGED_EVENT));
      onShared();
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  function openTeams() {
    onClose();
    onOpenTeams();
  }
</script>

<ModalShell open={true} {title} {onClose} width="md">
  <div class="modal-header">
    <h2 class="modal-title flex min-w-0 items-center gap-2">
      <Share2 size={16} class="shrink-0 text-muted" aria-hidden="true" />
      <span class="break-words">{title}</span>
    </h2>
    <button type="button" class="modal-close" onclick={onClose} aria-label="Close"><X size={16} /></button>
  </div>

  <div class="modal-body flex flex-col gap-3 text-sm text-secondary" bind:this={body}>
    {#if phase === "loading"}
      <div class="flex items-center gap-2 text-muted"><span class="spinner size-3"></span> Checking Teams…</div>
    {:else if phase === "not-set-up" || phase === "locked" || phase === "no-team" || phase === "error"}
      <div class="alert {phase === 'error' ? 'alert-error' : 'alert-info'}" role="status">
        <div class="alert-title flex items-center gap-1.5">
          {#if phase === "locked"}<Lock size={14} aria-hidden="true" />{:else}<Users size={14} aria-hidden="true" />{/if}
          {#if phase === "not-set-up"}
            Teams isn't set up on this device
          {:else if phase === "locked"}
            Teams is locked
          {:else if phase === "no-team" && ownTeam}
            This device can't write to {ownTeam.name} yet
          {:else if phase === "no-team"}
            No team can take this host yet
          {:else}
            Couldn't read Teams
          {/if}
        </div>
        <p class="m-0 text-xs leading-relaxed">
          {#if phase === "not-set-up"}
            Set up Teams, then create or join a team to share hosts with it.
          {:else if phase === "locked"}
            Unlock Teams with your passphrase to share hosts.
          {:else if phase === "no-team"}
            Sharing needs a team whose key an admin has granted to this device. Create or join a team, or ask an
            admin to grant this device after checking its fingerprint with you.
          {:else}
            {loadError}
          {/if}
        </p>
      </div>
    {:else}
      {#if ownTeam}
        <p class="m-0 leading-relaxed">
          This host is a copy from <strong class="text-primary">{ownTeam.name}</strong>. Your changes replace the
          team's version for everyone in it.
        </p>
      {:else}
        <div class="field">
          <label class="field-label" for="share-team-choice">Team</label>
          <select id="share-team-choice" class="input" bind:value={choice} data-autofocus>
            {#if teams.length > 1}<option value="">Choose a team…</option>{/if}
            {#each teams as t (t.team_id)}
              <option value={t.team_id}>{t.name}</option>
            {/each}
          </select>
        </div>
      {/if}

      <section aria-labelledby="share-what" class="flex flex-col gap-1.5">
        <h3 id="share-what" class="m-0 text-xs font-semibold text-primary">Shared</h3>
        <dl class="share-grid m-0 rounded-md border border-border p-2.5 text-xs">
          <dt>Host</dt>
          <dd class="font-mono">{connection.host}</dd>
          <dt>User</dt>
          <dd class="font-mono">{connection.user}</dd>
          <dt>Port</dt>
          <dd class="font-mono">{connection.port}</dd>
          <dt>Bastion</dt>
          <dd class="font-mono">
            {#if connection.bastion}{connection.bastion_user ? `${connection.bastion_user}@` : ""}{connection.bastion}{:else}<span class="text-muted">None</span>{/if}
          </dd>
          <dt>Kerberos</dt>
          <dd>{connection.use_kerberos ? "On" : "Off"}</dd>
          <dt>Tags</dt>
          <dd>{connection.tags.length ? connection.tags.join(", ") : "None"}</dd>
        </dl>
      </section>

      <p class="m-0 text-xs leading-relaxed">
        <strong class="text-primary">Never shared:</strong> key paths and passwords. Each member uses their own keys.
      </p>
      <p class="m-0 text-xs leading-relaxed">
        Everyone in {chosen?.name ?? "the team"} will see this host after their next sync, and connecting to it is
        recorded in the team's audit log.
      </p>

      {#if error}<div class="alert alert-error" role="alert">Couldn't share: {error}</div>{/if}
    {/if}
    <p class="m-0 text-xs text-muted">CLI: <code class="select-text">{cli}</code></p>
  </div>

  <div class="modal-footer">
    {#if phase === "ready"}
      <button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
      <button type="button" class="btn btn-primary" onclick={share} disabled={busy || !chosen} data-autofocus={ownTeam ? true : undefined}>
        {#if busy}<span class="spinner size-3"></span>{/if}
        {ownTeam ? `Update in ${ownTeam.name}` : chosen ? `Share with ${chosen.name}` : "Share"}
      </button>
    {:else if phase === "loading"}
      <button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onClose}>Close</button>
      <button type="button" class="btn btn-primary" onclick={openTeams} data-autofocus>
        <Users size={14} aria-hidden="true" />
        Open Teams
      </button>
    {/if}
  </div>
</ModalShell>

<style>
  .share-grid {
    display: grid;
    grid-template-columns: max-content 1fr;
    column-gap: 0.75rem;
    row-gap: 0.25rem;
  }
  .share-grid dt {
    color: var(--color-muted);
  }
  .share-grid dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--color-primary);
  }
</style>
