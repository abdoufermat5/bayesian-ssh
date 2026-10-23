<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Lock, LogIn, Plus, RefreshCw, Users } from "lucide-svelte";
  import type {
    Connection,
    OrgSummary,
    TeamAttention,
    TeamJoin,
    TeamsStatus,
    TeamSummary,
    TeamSyncReport,
    TeamSyncResult,
  } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import {
    getAttention,
    pendingTeamFocus,
    raiseAttention,
    resolveAttention,
    takeTeamFocus,
    takeTeamFocusTab,
    TEAM_ATTENTION,
  } from "$lib/stores/attention.svelte";
  import { TEAMS_CHANGED_EVENT } from "$lib/stores/teamsAttention.svelte";
  import TeamsSetup from "./teams/TeamsSetup.svelte";
  import TeamsUnlock from "./teams/TeamsUnlock.svelte";
  import TeamsHome from "./teams/TeamsHome.svelte";
  import TeamPage from "./teams/TeamPage.svelte";
  import OrgPage from "./teams/OrgPage.svelte";
  import CreateTeamModal from "./teams/CreateTeamModal.svelte";
  import JoinTeamModal from "./teams/JoinTeamModal.svelte";
  import { plural, type ParsedInvite, type TeamTab } from "./teams/teams";

  let {
    connections,
    onConnectionsChanged,
  }: { connections: Connection[]; onConnectionsChanged: () => void } = $props();

  /** How often `teams_attention` runs while this view is open and unlocked. */
  const ATTENTION_POLL_MS = 30_000;

  let status = $state.raw<TeamsStatus | null>(null);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let attention = $state.raw<Record<string, TeamAttention>>({});
  let lastSync = $state.raw<Record<string, TeamSyncReport>>({});
  /** Team id being synced, "all", or null. */
  let syncing = $state<string | null>(null);
  let openTeamId = $state<string | null>(null);
  let openTab = $state<TeamTab>("hosts");
  /** null until loaded, and when the server has no organizations (the UI is hidden). */
  let orgs = $state.raw<OrgSummary[] | null>(null);
  let openOrgId = $state<string | null>(null);
  let modal = $state<"create" | "join" | null>(null);
  /** Invite pasted during setup, to pre-fill the join once the device is set up. */
  let joinSeed = $state.raw<ParsedInvite | null>(null);

  const teams = $derived(status?.teams ?? []);
  const openOrg = $derived(orgs?.find((o) => o.org_id === openOrgId) ?? null);
  const openTeam = $derived(teams.find((t) => t.team_id === openTeamId) ?? null);
  const teamSourceIds = $derived(new Set(teams.map((t) => t.source_id)));
  const localConnections = $derived(connections.filter((c) => !c.source_id || !teamSourceIds.has(c.source_id)));

  function waitingForKey(teamId: string): boolean {
    return attention[teamId]?.waiting_for_key ?? lastSync[teamId]?.waiting_for_key ?? false;
  }

  async function refresh() {
    try {
      status = await invoke<TeamsStatus>("teams_status");
      loadError = null;
    } catch (err) {
      loadError = String(err);
    } finally {
      loading = false;
    }
  }

  async function loadOrgs() {
    if (!status?.unlocked) return;
    try {
      orgs = await invoke<OrgSummary[] | null>("teams_orgs");
    } catch {
      // Offline or an older app backend: keep what we had.
    }
  }

  // ── Attention: what needs a human, per team ──────────────────────────────

  function setWaitingKey(team: TeamSummary, waiting: boolean) {
    const id = TEAM_ATTENTION.waitingKey(team.team_id);
    if (!waiting) return resolveAttention(id);
    raiseAttention({
      id,
      severity: "info",
      title: `${team.name}: waiting for an admin to grant this device the key`,
      body: "Send them this device's fingerprint from the Teams page.",
      action: { label: "Show fingerprint", tab: "teams", teamId: team.team_id },
      dismissible: false,
    });
  }

  function applyAttention(list: TeamAttention[]) {
    const previous = attention;
    attention = Object.fromEntries(list.map((a) => [a.team_id, a]));
    for (const a of list) {
      const team = teams.find((t) => t.team_id === a.team_id);
      if (!team) continue;

      const pendingId = TEAM_ATTENTION.pending(team.team_id);
      if (a.role === "admin" && a.pending_devices > 0) {
        raiseAttention({
          id: pendingId,
          severity: "warning",
          title: `${team.name}: ${plural(a.pending_devices, "device")} waiting for the key`,
          body: "Check each fingerprint with its owner before granting.",
          action: { label: "Review", tab: "teams", teamId: team.team_id },
          dismissible: false,
        });
      } else {
        resolveAttention(pendingId);
      }

      setWaitingKey(team, a.waiting_for_key && !a.removed);

      const removedId = TEAM_ATTENTION.removed(team.team_id);
      if (a.removed) {
        raiseAttention({
          id: removedId,
          severity: "warning",
          title: `You were removed from ${team.name}`,
          body: "This device can't sync it anymore.",
          action: { label: "Open team", tab: "teams", teamId: team.team_id },
          dismissible: false,
        });
      } else {
        resolveAttention(removedId);
      }

      // The key just arrived: fetch the team's hosts right away.
      if (previous[a.team_id]?.waiting_for_key && !a.waiting_for_key && !a.removed && syncing === null) {
        notify(`${team.name}: key received, syncing`, "success");
        void syncTeam(team);
      }
    }
  }

  async function checkAttention() {
    if (!status?.unlocked || document.visibilityState === "hidden") return;
    try {
      applyAttention((await invoke<TeamAttention[] | null>("teams_attention")) ?? []);
    } catch {
      // Offline or an older backend: keep the last answer; the next poll retries.
    }
  }

  /** Sync results carry security events: they become persistent attention items. */
  function afterSync(team: TeamSummary, report: TeamSyncReport) {
    lastSync = { ...lastSync, [team.team_id]: report };
    setWaitingKey(team, report.waiting_for_key);

    const integrityId = TEAM_ATTENTION.integrity(team.team_id);
    if (report.rejected.length > 0) {
      raiseAttention({
        id: integrityId,
        severity: "error",
        title: `${team.name}: ${plural(report.rejected.length, "entry", "entries")} failed integrity checks`,
        body: "They were ignored. A server replaying old versions or an unknown author can cause this; tell your team admin.",
        action: { label: "Open team", tab: "teams", teamId: team.team_id },
      });
    } else {
      resolveAttention(integrityId);
    }

    const chainId = TEAM_ATTENTION.chainBroken(team.team_id);
    const chainBroken = report.errors.some((e) => e.includes("AuditChainBroken"));
    if (chainBroken) {
      raiseAttention({
        id: chainId,
        severity: "error",
        title: `${team.name}: the server's audit log is missing your events`,
        body: "Events this device uploaded before are gone from the server; they may have been deleted there. Tell your team admin.",
        action: { label: "Open team", tab: "teams", teamId: team.team_id },
      });
    } else {
      resolveAttention(chainId);
    }
    for (const e of report.errors) {
      if (!e.includes("AuditChainBroken")) notify(`${team.name}: ${e}`, "error");
    }
  }

  // ── Actions ─────────────────────────────────────────────────────────────

  function summarize(r: TeamSyncReport): string {
    if (r.waiting_for_key) return "waiting for an admin to grant this device the key";
    const parts = [`pulled ${r.pulled}`, `pushed ${r.pushed}`];
    if (r.hosts_added || r.hosts_updated || r.hosts_stale) {
      parts.push(`hosts +${r.hosts_added} ~${r.hosts_updated} stale ${r.hosts_stale}`);
    }
    if (r.audit_uploaded) parts.push(`${plural(r.audit_uploaded, "audit event")} uploaded`);
    if (r.rotated_to) parts.push(`key rotated to v${r.rotated_to}`);
    return parts.join(" · ");
  }

  async function runSync(key: string, team: string | null): Promise<TeamSyncResult[] | null> {
    syncing = key;
    try {
      return await invoke<TeamSyncResult[]>("teams_sync", { team });
    } catch (err) {
      notify(`Sync failed: ${err}`, "error");
      return null;
    } finally {
      syncing = null;
    }
  }

  async function syncAll() {
    const results = await runSync("all", null);
    if (!results) return;
    for (const r of results) {
      const team = teams.find((t) => t.team_id === r.result?.team_id || t.name === r.team);
      if (r.result && team) afterSync(team, r.result);
      if (r.error) notify(`${r.team}: ${r.error}`, "error");
    }
    const ok = results.filter((r) => r.result).length;
    if (ok) notify(`Synced ${plural(ok, "team")}`, "success");
    onConnectionsChanged();
    await refresh();
  }

  async function syncTeam(team: TeamSummary) {
    const r = (await runSync(team.team_id, team.team_id))?.[0];
    if (r?.result) {
      afterSync(team, r.result);
      notify(`${team.name}: ${summarize(r.result)}`, "success");
    }
    if (r?.error) notify(`${team.name}: ${r.error}`, "error");
    onConnectionsChanged();
    await refresh();
  }

  /** A share, unshare or remove ran on `team` and returned a sync report. */
  async function onTeamSynced(team: TeamSummary, report: TeamSyncReport) {
    afterSync(team, report);
    onConnectionsChanged();
    await refresh();
  }

  async function lock() {
    await invoke("teams_lock");
    attention = {};
    orgs = null;
    openOrgId = null;
    openTeamId = null;
    await refresh();
  }

  function openTeamPage(teamId: string, tab: TeamTab = "hosts") {
    openOrgId = null;
    openTab = tab;
    openTeamId = teamId;
  }

  async function onCreated(teamId: string, name: string) {
    modal = null;
    notify(`Created team '${name}'; you're its admin`, "success");
    await refresh();
    openTeamPage(teamId);
    void checkAttention();
    void loadOrgs();
  }

  async function onJoined(join: TeamJoin) {
    joinSeed = null;
    await refresh();
    const team = status?.teams.find((t) => t.team_id === join.team_id);
    if (team) setWaitingKey(team, true);
    void checkAttention();
    void loadOrgs();
  }

  // Organizations load once Teams is unlocked.
  $effect(() => {
    if (status?.unlocked) untrack(() => void loadOrgs());
  });

  // Poll while the view is open and unlocked (the view unmounts when hidden).
  $effect(() => {
    if (!status?.unlocked) return;
    untrack(() => void checkAttention());
    const timer = setInterval(() => void checkAttention(), ATTENTION_POLL_MS);
    return () => clearInterval(timer);
  });

  // Focus requests from attention actions elsewhere in the app.
  $effect(() => {
    if (!pendingTeamFocus() || !status?.unlocked) return;
    const teamId = takeTeamFocus();
    const requestedTab = takeTeamFocusTab();
    if (!teamId || !teams.some((t) => t.team_id === teamId)) return;
    const raised = new Set(getAttention().map((i) => i.id));
    const tab: TeamTab = requestedTab ?? raised.has(TEAM_ATTENTION.auditFailed(teamId))
      ? "audit"
      : raised.has(TEAM_ATTENTION.pending(teamId)) || raised.has(TEAM_ATTENTION.rotationBlocked(teamId))
        ? "members"
        : "hosts";
    modal = null;
    openTeamPage(teamId, tab);
  });

  onMount(() => {
    void refresh();
    // Sync, lock or share from the palette or the Hosts view.
    const onTeamsChanged = () => void refresh();
    window.addEventListener(TEAMS_CHANGED_EVENT, onTeamsChanged);
    return () => window.removeEventListener(TEAMS_CHANGED_EVENT, onTeamsChanged);
  });
</script>

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Teams</h1>
      {#if status?.unlocked}
        <span class="text-sm tabular-nums text-muted">{plural(teams.length, "team")}</span>
      {/if}
    </div>
    {#if status?.unlocked}
      <div class="view-actions">
        <button type="button" class="btn btn-secondary" onclick={() => (modal = "join")}>
          <LogIn size={14} />
          Join with invite
        </button>
        <button type="button" class="btn btn-secondary" onclick={() => (modal = "create")}>
          <Plus size={14} />
          Create team
        </button>
        {#if teams.length > 0}
          <button type="button" class="btn btn-secondary" onclick={syncAll} disabled={syncing !== null}>
            <RefreshCw size={14} class={syncing === "all" ? "animate-spin" : ""} />
            Sync all
          </button>
        {/if}
        <button
          type="button"
          class="btn btn-ghost"
          onclick={lock}
          disabled={syncing !== null}
          title="Forget the unlocked keys until you unlock again"
        >
          <Lock size={14} />
          Lock
        </button>
      </div>
    {/if}
  </header>

  <div class="view-body">
    {#if loading}
      <div class="flex flex-col gap-3" aria-busy="true" aria-label="Loading Teams">
        <div class="skeleton h-24 w-full rounded-md"></div>
        <div class="skeleton h-16 w-full rounded-md"></div>
      </div>
    {:else if !status}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Users size={18} /></div>
        <div class="empty-state-title">Teams couldn't load</div>
        <p class="empty-state-desc">{loadError}</p>
      </div>
    {:else if !status.identity}
      <TeamsSetup
        onDone={(next, invite) => {
          status = next;
          if (invite) {
            joinSeed = invite;
            modal = "join";
          }
        }}
      />
    {:else if !status.unlocked}
      <TeamsUnlock identity={status.identity} onUnlocked={(next) => (status = next)} />
    {:else if openOrg}
      <OrgPage
        org={openOrg}
        onBack={() => (openOrgId = null)}
        onOpenTeam={(id) => openTeamPage(id)}
        onChanged={() => void loadOrgs()}
      />
    {:else if openTeam}
      {#key openTeam.team_id}
        <TeamPage
          team={openTeam}
          attentionRole={attention[openTeam.team_id]?.role ?? null}
          pendingDevices={attention[openTeam.team_id]?.pending_devices ?? 0}
          waitingForKey={waitingForKey(openTeam.team_id)}
          removed={!!attention[openTeam.team_id]?.removed}
          myEmail={status.identity.email}
          myFingerprint={status.fingerprint}
          teamConnections={connections.filter((c) => c.source_id === openTeam.source_id)}
          {localConnections}
          syncing={syncing !== null}
          initialTab={openTab}
          onBack={() => (openTeamId = null)}
          onSync={() => syncTeam(openTeam)}
          onSynced={(report) => onTeamSynced(openTeam, report)}
          onPendingChanged={(count) => {
            const a = attention[openTeam.team_id];
            if (a && a.role === "admin" && a.pending_devices !== count) {
              applyAttention(Object.values({ ...attention, [a.team_id]: { ...a, pending_devices: count } }));
            }
          }}
        />
      {/key}
    {:else}
      <TeamsHome
        {status}
        {orgs}
        {attention}
        {waitingForKey}
        {syncing}
        onOpen={(team) => openTeamPage(team.team_id)}
        onSync={syncTeam}
        onCreate={() => (modal = "create")}
        onJoin={() => (modal = "join")}
        onOpenOrg={(org) => (openOrgId = org.org_id)}
      />
    {/if}
  </div>
</div>

{#if status?.unlocked && status.identity && modal === "create"}
  <CreateTeamModal {orgs} onClose={() => (modal = null)} {onCreated} />
{:else if status?.unlocked && status.identity && modal === "join"}
  <JoinTeamModal
    identity={status.identity}
    initial={joinSeed}
    onClose={() => {
      modal = null;
      joinSeed = null;
    }}
    {onJoined}
  />
{/if}
