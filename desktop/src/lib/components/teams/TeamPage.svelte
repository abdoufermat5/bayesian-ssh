<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ArrowLeft, RefreshCw } from "lucide-svelte";
  import type { Connection, TeamMember, TeamRole, TeamSummary, TeamSyncReport } from "$lib/types";
  import { getAttention, TEAM_ATTENTION } from "$lib/stores/attention.svelte";
  import { formatRelative } from "$lib/utils/timezone";
  import TeamStatusNotices from "./TeamStatusNotices.svelte";
  import TeamHostsTab from "./TeamHostsTab.svelte";
  import TeamMembersTab from "./TeamMembersTab.svelte";
  import TeamAuditTab from "./TeamAuditTab.svelte";
  import { formatUtc, plural, type TeamTab } from "./teams";

  let {
    team,
    attentionRole,
    pendingDevices,
    waitingForKey,
    removed,
    myEmail,
    myFingerprint,
    teamConnections,
    localConnections,
    syncing,
    initialTab = "hosts",
    onBack,
    onSync,
    onSynced,
    onPendingChanged,
  }: {
    team: TeamSummary;
    /** From `teams_attention`; null until it answers (or if it failed). */
    attentionRole: TeamRole | null;
    pendingDevices: number;
    waitingForKey: boolean;
    removed: boolean;
    myEmail: string;
    myFingerprint: string | undefined;
    teamConnections: Connection[];
    localConnections: Connection[];
    syncing: boolean;
    initialTab?: TeamTab;
    onBack: () => void;
    onSync: () => void;
    onSynced: (report: TeamSyncReport) => void;
    onPendingChanged: (count: number) => void;
  } = $props();

  let members = $state.raw<TeamMember[] | null>(null);
  let membersError = $state<string | null>(null);
  // Seeded from the prop once; the user owns it afterwards.
  let tab = $state<TeamTab>(untrack(() => initialTab));

  /** `teams_attention` is authoritative; the member list is a fallback when it can't answer. */
  const role = $derived<TeamRole | null>(
    attentionRole ?? members?.find((m) => m.email === myEmail)?.role ?? null,
  );
  const isAdmin = $derived(role === "admin");
  const tabs = $derived<{ id: TeamTab; label: string }[]>([
    { id: "hosts", label: "Hosts" },
    { id: "members", label: "Members & devices" },
    ...(isAdmin ? [{ id: "audit" as const, label: "Audit log" }] : []),
  ]);
  const activeTab = $derived(tabs.some((t) => t.id === tab) ? tab : "hosts");

  /** Persistent problems for this team; the audit tab shows its own failure in full. */
  const problems = $derived(
    getAttention().filter(
      (item) =>
        item.severity === "error" &&
        item.id.startsWith("teams:") &&
        item.id.endsWith(`:${team.team_id}`) &&
        !(activeTab === "audit" && item.id === TEAM_ATTENTION.auditFailed(team.team_id)),
    ),
  );

  async function loadMembers() {
    membersError = null;
    try {
      members = await invoke<TeamMember[]>("teams_members", { team: team.team_id });
    } catch (err) {
      membersError = String(err);
    }
  }

  async function onTabKeydown(e: KeyboardEvent) {
    const index = tabs.findIndex((t) => t.id === activeTab);
    let next = index;
    if (e.key === "ArrowRight") next = (index + 1) % tabs.length;
    else if (e.key === "ArrowLeft") next = (index - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    else return;
    e.preventDefault();
    tab = tabs[next].id;
    await tick();
    document.getElementById(`team-tab-${tab}`)?.focus();
  }

  onMount(() => {
    void loadMembers();
  });
</script>

<div class="flex flex-col gap-4">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <div class="flex min-w-0 flex-wrap items-center gap-2.5">
      <button type="button" class="btn btn-ghost btn-sm" onclick={onBack}>
        <ArrowLeft size={14} />
        All teams
      </button>
      <h2 class="m-0 truncate text-lg font-semibold text-primary">{team.name}</h2>
      {#if role}
        <span class="badge {isAdmin ? 'badge-accent' : 'badge-neutral'}">{isAdmin ? "Admin" : "Member"}</span>
      {/if}
      <span class="text-xs text-muted">{plural(team.hosts, "host")}</span>
      <span class="text-xs text-muted" title={team.last_sync_at ? formatUtc(team.last_sync_at, true) : undefined}>
        {team.last_sync_at ? `synced ${formatRelative(team.last_sync_at)}` : "never synced"}
      </span>
    </div>
    <button type="button" class="btn btn-secondary btn-sm" onclick={onSync} disabled={syncing}>
      <RefreshCw size={14} class={syncing ? "animate-spin" : ""} />
      Sync
    </button>
  </div>

  <TeamStatusNotices {team} {waitingForKey} {removed} {myFingerprint} />

  {#each problems as item (item.id)}
    <div class="alert alert-error" role="alert">
      <div class="flex flex-col gap-0.5">
        <span class="alert-title">{item.title}</span>
        {#if item.body}<span>{item.body}</span>{/if}
      </div>
    </div>
  {/each}

  <div class="tabs" role="tablist" aria-label="{team.name} sections">
    {#each tabs as t (t.id)}
      <button
        type="button"
        id="team-tab-{t.id}"
        class="tab {activeTab === t.id ? 'tab-active' : ''}"
        role="tab"
        aria-selected={activeTab === t.id}
        aria-controls="team-panel"
        tabindex={activeTab === t.id ? 0 : -1}
        onclick={() => (tab = t.id)}
        onkeydown={onTabKeydown}
      >
        {t.label}
        {#if t.id === "members" && isAdmin && pendingDevices > 0}
          <span class="badge badge-warning">{pendingDevices}</span>
        {/if}
      </button>
    {/each}
  </div>

  <div id="team-panel" role="tabpanel" aria-labelledby="team-tab-{activeTab}" tabindex="-1">
    {#if activeTab === "hosts"}
      <TeamHostsTab {team} {teamConnections} {localConnections} {onSynced} />
    {:else if activeTab === "members"}
      <TeamMembersTab
        {team}
        {isAdmin}
        {myEmail}
        {members}
        {membersError}
        pendingHint={pendingDevices}
        onMembersChanged={loadMembers}
        onRemoved={onSynced}
        {onPendingChanged}
      />
    {:else}
      <TeamAuditTab {team} {members} {teamConnections} />
    {/if}
  </div>
</div>
