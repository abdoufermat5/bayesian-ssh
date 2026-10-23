<script lang="ts">
  import { Building2, LogIn, Plus, RefreshCw, Users } from "lucide-svelte";
  import type { OrgSummary, TeamAttention, TeamsStatus, TeamSummary } from "$lib/types";
  import { getAttention, TEAM_ATTENTION } from "$lib/stores/attention.svelte";
  import { formatRelative } from "$lib/utils/timezone";
  import Fingerprint from "./Fingerprint.svelte";
  import TeamStatusNotices from "./TeamStatusNotices.svelte";
  import { formatUtc, plural } from "./teams";

  let {
    status,
    orgs = null,
    attention,
    waitingForKey,
    syncing,
    onOpen,
    onSync,
    onCreate,
    onJoin,
    onOpenOrg,
  }: {
    status: TeamsStatus;
    /** null when the server has no organizations: that section is hidden. */
    orgs?: OrgSummary[] | null;
    attention: Record<string, TeamAttention>;
    /** Teams whose last sync or attention check says this device has no key yet. */
    waitingForKey: (teamId: string) => boolean;
    /** Team id being synced, "all", or null. */
    syncing: string | null;
    onOpen: (team: TeamSummary) => void;
    onSync: (team: TeamSummary) => void;
    onCreate: () => void;
    onJoin: () => void;
    onOpenOrg?: (org: OrgSummary) => void;
  } = $props();

  type Flag = { label: string; tone: "error" | "warning" | "accent" };

  const ERROR_FLAGS: [(id: string) => string, string][] = [
    [TEAM_ATTENTION.auditFailed, "Audit log failed"],
    [TEAM_ATTENTION.integrity, "Integrity problems"],
    [TEAM_ATTENTION.chainBroken, "Audit chain broken"],
    [TEAM_ATTENTION.rotationBlocked, "Rotation stopped"],
  ];

  const raised = $derived(new Set(getAttention().map((i) => i.id)));

  function flagsFor(team: TeamSummary): Flag[] {
    const a = attention[team.team_id];
    const flags: Flag[] = ERROR_FLAGS.filter(([id]) => raised.has(id(team.team_id))).map(([, label]) => ({
      label,
      tone: "error",
    }));
    if (a?.removed) flags.push({ label: "You were removed", tone: "warning" });
    else if (waitingForKey(team.team_id)) flags.push({ label: "Waiting for the key", tone: "accent" });
    if (a?.role === "admin" && a.pending_devices > 0) {
      flags.push({ label: `${plural(a.pending_devices, "device")} waiting`, tone: "warning" });
    }
    return flags;
  }
</script>

<div class="flex flex-col gap-4">
  {#if status.identity}
    <section class="panel flex flex-col gap-2 p-4" aria-label="This device">
      <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span class="font-medium text-primary">{status.identity.email}</span>
        <span class="type-meta">{status.identity.server_url}</span>
        <span class="type-meta">Unlocked until you lock or quit</span>
      </div>
      {#if status.fingerprint}
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span class="type-meta">This device</span>
          <Fingerprint value={status.fingerprint} label="this device's fingerprint" />
        </div>
        <p class="m-0 type-meta">
          Admins compare this fingerprint with what the server reports before granting you a team key. Send it over a
          channel where they know it's you.
        </p>
      {/if}
    </section>
  {/if}

  {#each status.teams as team (team.team_id)}
    <TeamStatusNotices
      {team}
      waitingForKey={waitingForKey(team.team_id)}
      removed={!!attention[team.team_id]?.removed}
      myFingerprint={status.fingerprint}
      showName
    />
  {/each}

  {#if status.teams.length === 0}
    <div class="empty-state py-12">
      <div class="empty-state-icon"><Users size={18} /></div>
      <div class="empty-state-title">No teams yet. Create one to share hosts, or join with an invite.</div>
      <div class="empty-state-action flex gap-2">
        <button type="button" class="btn btn-secondary" onclick={onJoin}><LogIn size={14} /> Join with invite</button>
        <button type="button" class="btn btn-primary" onclick={onCreate}><Plus size={14} /> Create team</button>
      </div>
    </div>
  {:else}
    <div class="table-wrap overflow-x-auto">
      <table class="data-table">
        <thead>
          <tr>
            <th>Team</th>
            <th>Role</th>
            <th>Hosts</th>
            <th>Synced</th>
            <th>Needs attention</th>
            <th><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each status.teams as team (team.team_id)}
            {@const role = attention[team.team_id]?.role}
            {@const flags = flagsFor(team)}
            <tr>
              <td>
                <button type="button" class="link font-medium" onclick={() => onOpen(team)}>{team.name}</button>
              </td>
              <td>
                {#if role}
                  <span class="badge {role === 'admin' ? 'badge-accent' : 'badge-neutral'}">{role === "admin" ? "Admin" : "Member"}</span>
                {:else}
                  <span class="text-muted">–</span>
                {/if}
              </td>
              <td class="tabular-nums">{team.hosts}</td>
              <td>
                {#if team.last_sync_at}
                  <span title={formatUtc(team.last_sync_at, true)}>{formatRelative(team.last_sync_at)}</span>
                {:else}
                  <span class="text-muted">never</span>
                {/if}
              </td>
              <td>
                {#if flags.length}
                  <span class="flex flex-wrap gap-1">
                    {#each flags as f (f.label)}<span class="badge badge-{f.tone}">{f.label}</span>{/each}
                  </span>
                {:else}
                  <span class="text-muted">–</span>
                {/if}
              </td>
              <td class="text-right">
                <button
                  type="button"
                  class="btn btn-ghost btn-sm"
                  onclick={() => onSync(team)}
                  disabled={syncing !== null}
                  aria-label="Sync {team.name}"
                >
                  <RefreshCw size={14} class={syncing === team.team_id ? "animate-spin" : ""} />
                  Sync
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  {#if orgs && orgs.length > 0}
    <section class="flex flex-col gap-2" aria-labelledby="orgs-heading">
      <h3 id="orgs-heading" class="section-heading flex items-center gap-1.5">
        <Building2 size={14} /> Organizations
      </h3>
      <div class="table-wrap overflow-x-auto">
        <table class="data-table">
          <thead>
            <tr>
              <th>Organization (owners)</th>
              <th>Role</th>
              <th>Teams</th>
            </tr>
          </thead>
          <tbody>
            {#each orgs as org (org.org_id)}
              <tr>
                <td>
                  <button type="button" class="link font-medium" onclick={() => onOpenOrg?.(org)}>{org.label}</button>
                </td>
                <td>
                  <span class="badge {org.my_role === 'owner' ? 'badge-accent' : 'badge-neutral'}">
                    {org.my_role === "owner" ? "Owner" : "Member"}
                  </span>
                </td>
                <td class="tabular-nums">{org.teams.length}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {/if}
</div>
