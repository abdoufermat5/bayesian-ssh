<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Hourglass, KeyRound, RefreshCw, UserMinus, UserPlus } from "lucide-svelte";
  import type { TeamMember, TeamPendingDevice, TeamSummary, TeamSyncReport } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import Fingerprint from "./Fingerprint.svelte";
  import GrantDeviceModal from "./GrantDeviceModal.svelte";
  import InviteModal from "./InviteModal.svelte";
  import RemoveMemberModal from "./RemoveMemberModal.svelte";

  let {
    team,
    isAdmin,
    myEmail,
    members,
    membersError,
    pendingHint,
    onMembersChanged,
    onRemoved,
    onPendingChanged,
  }: {
    team: TeamSummary;
    isAdmin: boolean;
    myEmail: string;
    /** null while loading. */
    members: TeamMember[] | null;
    membersError: string | null;
    /** Waiting-device count from `teams_attention`; a change reloads the list. */
    pendingHint: number;
    onMembersChanged: () => void;
    onRemoved: (report: TeamSyncReport) => void;
    onPendingChanged: (count: number) => void;
  } = $props();

  let pending = $state.raw<TeamPendingDevice[] | null>(null);
  let pendingError = $state<string | null>(null);
  let pendingBusy = $state(false);
  let granting = $state.raw<TeamPendingDevice | null>(null);
  let removing = $state<string | null>(null);
  let inviting = $state(false);

  /**
   * Server-reported fingerprints of devices still waiting for the key. They're
   * hidden everywhere, so the grant check can't be answered from the screen.
   */
  const pendingFingerprints = $derived(new Set((pending ?? []).map((p) => p.device.fingerprint)));

  async function loadPending() {
    pendingBusy = true;
    pendingError = null;
    try {
      pending = await invoke<TeamPendingDevice[]>("teams_pending", { team: team.team_id });
      onPendingChanged(pending.length);
    } catch (err) {
      pendingError = String(err);
    } finally {
      pendingBusy = false;
    }
  }

  $effect(() => {
    // Reload when the role becomes known or the poller reports a new count.
    void pendingHint;
    if (isAdmin) void loadPending();
  });
</script>

<div class="flex flex-col gap-6">
  {#if isAdmin}
    <section class="flex flex-col gap-2" aria-labelledby="team-pending-heading">
      <div class="flex items-center justify-between gap-2">
        <h3 id="team-pending-heading" class="section-heading flex items-center gap-1.5">
          <KeyRound size={14} /> Devices waiting for the key
          {#if pending}<span class="count ml-1">{pending.length}</span>{/if}
        </h3>
        <button type="button" class="btn btn-ghost btn-sm" onclick={loadPending} disabled={pendingBusy}>
          <RefreshCw size={14} class={pendingBusy ? "animate-spin" : ""} />
          Check again
        </button>
      </div>
      {#if pendingError}
        <div class="alert alert-error" role="alert">Couldn't load waiting devices: {pendingError}</div>
      {:else if pending === null}
        <div class="skeleton h-10 w-full rounded-md" aria-label="Loading waiting devices"></div>
      {:else if pending.length === 0}
        <p class="m-0 type-meta">No device is waiting.</p>
      {:else}
        <ul class="m-0 flex list-none flex-col gap-2 p-0">
          {#each pending as p (p.device.fingerprint)}
            <li class="flex flex-wrap items-center justify-between gap-2 rounded-md border border-warning/30 bg-warning/8 px-3 py-2">
              <span class="flex min-w-0 items-center gap-2 type-body">
                <Hourglass size={14} class="shrink-0 text-warning" />
                <span class="text-primary">{p.email}</span>
                <span class="text-muted">·</span>
                <span class="text-secondary">{p.device.name || "device"}</span>
              </span>
              <button type="button" class="btn btn-secondary btn-sm" onclick={() => (granting = p)}>
                Verify and grant…
              </button>
            </li>
          {/each}
        </ul>
        <p class="m-0 type-meta">
          You'll need the fingerprint each person sends you over a channel you trust; it isn't shown here.
        </p>
      {/if}
    </section>
  {/if}

  <section class="flex flex-col gap-2" aria-labelledby="team-members-heading">
    <div class="flex items-center justify-between gap-2">
      <h3 id="team-members-heading" class="section-heading">
        Members {#if members}<span class="count ml-1">{members.length}</span>{/if}
      </h3>
      {#if isAdmin}
        <button type="button" class="btn btn-secondary btn-sm" onclick={() => (inviting = true)}>
          <UserPlus size={14} />
          Invite
        </button>
      {/if}
    </div>
    {#if membersError}
      <div class="alert alert-error" role="alert">Couldn't load members: {membersError}</div>
    {:else if members === null}
      <div class="skeleton h-16 w-full rounded-md" aria-label="Loading members"></div>
    {:else}
      <ul class="panel m-0 flex list-none flex-col p-0">
        {#each members as m, i (m.email)}
          <li class="flex flex-col gap-1.5 px-4 py-3" class:border-t={i > 0} class:border-border-subtle={i > 0}>
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="flex min-w-0 items-center gap-2 type-body">
                <span class="truncate text-primary">{m.email}</span>
                <span class="badge badge-neutral">{m.role === "admin" ? "Admin" : "Member"}</span>
                {#if m.email === myEmail}<span class="type-meta">you</span>{/if}
              </span>
              {#if isAdmin && m.email !== myEmail}
                <button type="button" class="btn btn-ghost btn-sm" onclick={() => (removing = m.email)} aria-label="Remove {m.email}">
                  <UserMinus size={14} />
                  Remove…
                </button>
              {/if}
            </div>
            {#each m.devices as d (d.fingerprint)}
              <div class="flex flex-wrap items-center gap-x-3 gap-y-1 pl-3 text-xs text-secondary">
                <span class="min-w-24">{d.name || "device"}</span>
                {#if pendingFingerprints.has(d.fingerprint)}
                  <span class="badge badge-warning"><Hourglass size={12} /> Waiting for the key</span>
                {:else if isAdmin && pending === null}
                  <span class="text-muted">Checking…</span>
                {:else}
                  <Fingerprint value={d.fingerprint} copyable={false} />
                {/if}
              </div>
            {/each}
          </li>
        {/each}
      </ul>
      {#if !isAdmin}
        <p class="m-0 type-meta">Only admins can invite people, grant keys and remove members.</p>
      {/if}
    {/if}
  </section>
</div>

{#if granting}
  <GrantDeviceModal
    {team}
    device={granting}
    onClose={() => (granting = null)}
    onGranted={(result) => {
      notify(`Granted the ${team.name} key to ${result.email}`, "success");
      granting = null;
      void loadPending();
      onMembersChanged();
    }}
  />
{/if}

{#if removing}
  <RemoveMemberModal
    {team}
    email={removing}
    onClose={() => (removing = null)}
    onRemoved={(report) => {
      notify(`Removed ${removing}${report.rotated_to ? `; team key rotated to v${report.rotated_to}` : ""}`, "success");
      removing = null;
      onRemoved(report);
      onMembersChanged();
    }}
  />
{/if}

{#if inviting}
  <InviteModal {team} onClose={() => (inviting = false)} />
{/if}
