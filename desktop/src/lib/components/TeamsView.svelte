<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import {
    Check,
    ChevronDown,
    ChevronRight,
    Copy,
    KeyRound,
    Lock,
    LogIn,
    Plus,
    RefreshCw,
    Share2,
    Trash2,
    UserPlus,
    Users,
  } from "lucide-svelte";
  import type {
    Connection,
    TeamInvite,
    TeamJoin,
    TeamMember,
    TeamPendingDevice,
    TeamsStatus,
    TeamSummary,
    TeamSyncReport,
    TeamSyncResult,
  } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";

  let {
    connections,
    onConnectionsChanged,
  }: { connections: Connection[]; onConnectionsChanged: () => void } = $props();

  let status = $state.raw<TeamsStatus | null>(null);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  /** Key of the action in flight, e.g. "sync:<team>"; one at a time. */
  let busy = $state<string | null>(null);

  // Setup and unlock.
  let server = $state("");
  let email = $state("");
  let deviceName = $state("");
  let pass1 = $state("");
  let pass2 = $state("");
  let unlockPass = $state("");

  // Create and join.
  let newTeam = $state("");
  let joinToken = $state("");
  let joinTeamId = $state("");
  let joinFingerprint = $state("");
  let joined = $state.raw<TeamJoin | null>(null);

  // Per-team panels, keyed by team id.
  let expanded = $state<string | null>(null);
  let members = $state.raw<Record<string, TeamMember[]>>({});
  let pending = $state.raw<Record<string, TeamPendingDevice[]>>({});
  let grantInput = $state<Record<string, string>>({});
  let inviteEmail = $state<Record<string, string>>({});
  let inviteAdmin = $state<Record<string, boolean>>({});
  let invites = $state.raw<Record<string, TeamInvite>>({});
  let shareChoice = $state<Record<string, string>>({});
  let lastSync = $state.raw<Record<string, TeamSyncReport>>({});
  let confirmUnshare = $state<string | null>(null);
  let confirmRemove = $state<string | null>(null);
  let copied = $state<string | null>(null);

  const teamSourceIds = $derived(new Set((status?.teams ?? []).map((t) => t.source_id)));
  const localConnections = $derived(
    connections.filter((c) => !c.source_id || !teamSourceIds.has(c.source_id)),
  );

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

  /** Run one action, report failures, and keep the buttons disabled meanwhile. */
  async function run<T>(key: string, action: () => Promise<T>, failure: string): Promise<T | undefined> {
    busy = key;
    try {
      return await action();
    } catch (err) {
      notify(`${failure}: ${err}`, "error");
      return undefined;
    } finally {
      busy = null;
    }
  }

  function copy(text: string, id: string) {
    copyTextWithFallback(text);
    copied = id;
    notify("Copied", "success");
    setTimeout(() => {
      if (copied === id) copied = null;
    }, 2000);
  }

  function summarize(r: TeamSyncReport): string {
    if (r.waiting_for_key) return "Waiting for an admin to grant this device the team key";
    const parts = [`pulled ${r.pulled}`, `pushed ${r.pushed}`];
    if (r.hosts_added || r.hosts_updated || r.hosts_stale) {
      parts.push(`hosts +${r.hosts_added} ~${r.hosts_updated} stale ${r.hosts_stale}`);
    }
    if (r.audit_uploaded) parts.push(`${r.audit_uploaded} audit event(s) uploaded`);
    if (r.rotated_to) parts.push(`key rotated to v${r.rotated_to}`);
    return parts.join(" · ");
  }

  function afterSync(teamId: string, report: TeamSyncReport) {
    lastSync = { ...lastSync, [teamId]: report };
    if (report.rejected.length > 0) {
      notify(`${report.rejected.length} entries failed integrity checks and were ignored`, "error");
    }
    for (const e of report.errors) notify(e, "error");
  }

  async function setUp() {
    if (pass1 !== pass2) {
      notify("The passphrases don't match", "error");
      return;
    }
    const next = await run(
      "setup",
      () =>
        invoke<TeamsStatus>("teams_init", {
          server: server.trim(),
          email: email.trim(),
          deviceName: deviceName.trim() || null,
          passphrase: pass1,
        }),
      "Setup failed",
    );
    if (next) {
      status = next;
      pass1 = pass2 = "";
      notify("Teams is set up on this device", "success");
    }
  }

  async function unlock() {
    const next = await run("unlock", () => invoke<TeamsStatus>("teams_unlock", { passphrase: unlockPass }), "Couldn't unlock");
    if (next) {
      status = next;
      unlockPass = "";
    }
  }

  async function lock() {
    await invoke("teams_lock");
    members = {};
    pending = {};
    invites = {};
    await refresh();
  }

  async function syncAll() {
    const results = await run("sync:all", () => invoke<TeamSyncResult[]>("teams_sync", { team: null }), "Sync failed");
    if (!results) return;
    for (const r of results) {
      if (r.result) afterSync(r.result.team_id, r.result);
      if (r.error) notify(`${r.team}: ${r.error}`, "error");
    }
    const ok = results.filter((r) => r.result).length;
    if (ok) notify(`Synced ${ok} ${ok === 1 ? "team" : "teams"}`, "success");
    onConnectionsChanged();
    await refresh();
  }

  async function syncTeam(team: TeamSummary) {
    const results = await run(`sync:${team.team_id}`, () => invoke<TeamSyncResult[]>("teams_sync", { team: team.team_id }), "Sync failed");
    const r = results?.[0];
    if (r?.result) {
      afterSync(team.team_id, r.result);
      notify(`${r.result.name ?? team.name}: ${summarize(r.result)}`, "success");
    }
    if (r?.error) notify(`${team.name}: ${r.error}`, "error");
    onConnectionsChanged();
    await refresh();
  }

  async function createTeam() {
    const name = newTeam.trim();
    const report = await run("create", () => invoke<TeamSyncReport>("teams_create", { name }), "Couldn't create the team");
    if (report) {
      newTeam = "";
      notify(`Created team '${name}'; you're its admin`, "success");
      await refresh();
      expanded = report.team_id;
    }
  }

  async function join() {
    const result = await run(
      "join",
      () =>
        invoke<TeamJoin>("teams_join", {
          token: joinToken.trim() || null,
          teamId: joinTeamId.trim() || null,
          fingerprint: joinFingerprint.trim(),
        }),
      "Couldn't join",
    );
    if (result) {
      joined = result;
      joinToken = joinTeamId = joinFingerprint = "";
      await refresh();
    }
  }

  async function share(team: TeamSummary) {
    const connection = shareChoice[team.team_id];
    if (!connection) return;
    const report = await run(`share:${team.team_id}`, () => invoke<TeamSyncReport>("teams_share", { connection, team: team.team_id }), "Couldn't share");
    if (report) {
      afterSync(team.team_id, report);
      notify(`Shared '${connection}' with ${team.name}`, "success");
      shareChoice = { ...shareChoice, [team.team_id]: "" };
      onConnectionsChanged();
      await refresh();
    }
  }

  async function unshare(team: TeamSummary, connection: Connection) {
    if (confirmUnshare !== connection.id) {
      confirmUnshare = connection.id;
      return;
    }
    confirmUnshare = null;
    const report = await run(`unshare:${connection.id}`, () => invoke<TeamSyncReport>("teams_unshare", { connection: connection.name }), "Couldn't remove it");
    if (report) {
      afterSync(team.team_id, report);
      notify(`Removed '${connection.name}' from ${team.name} for everyone`, "success");
      onConnectionsChanged();
      await refresh();
    }
  }

  async function loadMembers(team: TeamSummary) {
    const list = await run(`members:${team.team_id}`, () => invoke<TeamMember[]>("teams_members", { team: team.team_id }), "Couldn't load members");
    if (list) members = { ...members, [team.team_id]: list };
  }

  async function removeMember(team: TeamSummary, member: TeamMember) {
    const key = `${team.team_id}:${member.email}`;
    if (confirmRemove !== key) {
      confirmRemove = key;
      return;
    }
    confirmRemove = null;
    const report = await run(`remove:${key}`, () => invoke<TeamSyncReport>("teams_remove", { team: team.team_id, email: member.email }), "Couldn't remove the member");
    if (report) {
      afterSync(team.team_id, report);
      notify(
        `Removed ${member.email}${report.rotated_to ? `; team key rotated to v${report.rotated_to}` : ""}`,
        "success",
      );
      await loadMembers(team);
    }
  }

  async function invite(team: TeamSummary) {
    const address = (inviteEmail[team.team_id] ?? "").trim();
    const result = await run(
      `invite:${team.team_id}`,
      () => invoke<TeamInvite>("teams_invite", { team: team.team_id, email: address, admin: !!inviteAdmin[team.team_id] }),
      "Couldn't invite",
    );
    if (result) {
      invites = { ...invites, [team.team_id]: result };
      inviteEmail = { ...inviteEmail, [team.team_id]: "" };
    }
  }

  function inviteText(i: TeamInvite): string {
    return [
      `bssh teams init --server ${i.server_url} --email ${i.email}`,
      `bssh teams join ${i.token} --fingerprint ${i.admin_fingerprint}`,
    ].join("\n");
  }

  async function loadPending(team: TeamSummary) {
    const list = await run(`pending:${team.team_id}`, () => invoke<TeamPendingDevice[]>("teams_pending", { team: team.team_id }), "Couldn't load waiting devices");
    if (list) pending = { ...pending, [team.team_id]: list };
  }

  async function grant(team: TeamSummary, device: TeamPendingDevice) {
    const result = await run(
      `grant:${device.device.fingerprint}`,
      () => invoke<TeamPendingDevice>("teams_grant", { team: team.team_id, fingerprint: device.device.fingerprint }),
      "Couldn't grant the key",
    );
    if (result) {
      notify(`Granted the ${team.name} key to ${result.email}`, "success");
      grantInput = { ...grantInput, [device.device.fingerprint]: "" };
      await loadPending(team);
    }
  }

  function toggle(team: TeamSummary) {
    expanded = expanded === team.team_id ? null : team.team_id;
    if (expanded) {
      void loadMembers(team);
    }
  }

  onMount(() => {
    void refresh();
  });
</script>

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Teams</h1>
      {#if status?.unlocked}
        <span class="text-sm tabular-nums text-muted">
          {status.teams.length} {status.teams.length === 1 ? "team" : "teams"}
        </span>
      {/if}
    </div>
    {#if status?.unlocked}
      <div class="view-actions">
        {#if status.teams.length > 0}
          <button type="button" class="btn btn-secondary" onclick={syncAll} disabled={busy !== null}>
            <RefreshCw size={14} class={busy === "sync:all" ? "animate-spin" : ""} />
            Sync all
          </button>
        {/if}
        <button type="button" class="btn btn-ghost" onclick={lock} disabled={busy !== null} title="Forget the unlocked keys until you unlock again">
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
      <!-- Not set up yet -->
      <div class="panel mx-auto flex max-w-xl flex-col gap-3 p-5">
        <div class="flex items-center gap-2">
          <Users size={16} />
          <h2 class="font-semibold">Share connections with your team</h2>
        </div>
        <p class="text-sm text-muted">
          Team connections are encrypted on your devices; the sync server only ever stores ciphertext.
          Set this device up once, then create a team or join one you were invited to.
        </p>
        <label class="flex flex-col gap-1 text-sm">
          Sync server
          <input class="input input-mono" bind:value={server} placeholder="https://teams.example.com" />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          Your email
          <input class="input" type="email" bind:value={email} placeholder="you@example.com" />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          Device name <span class="text-xs text-muted">shown to team admins (default: this machine's name)</span>
          <input class="input" bind:value={deviceName} placeholder="laptop" />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          Passphrase <span class="text-xs text-muted">protects this device's keys; at least 8 characters</span>
          <input class="input" type="password" bind:value={pass1} autocomplete="new-password" />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          Repeat passphrase
          <input class="input" type="password" bind:value={pass2} autocomplete="new-password" />
        </label>
        <div>
          <button
            type="button"
            class="btn btn-primary"
            onclick={setUp}
            disabled={busy !== null || !server.trim() || !email.trim() || pass1.length < 8 || pass1 !== pass2}
          >
            {#if busy === "setup"}<span class="spinner size-3"></span>{/if}
            Set up this device
          </button>
        </div>
      </div>
    {:else if !status.unlocked}
      <!-- Set up, locked -->
      <div class="panel mx-auto flex max-w-md flex-col gap-3 p-5">
        <div class="flex items-center gap-2">
          <Lock size={16} />
          <h2 class="font-semibold">Unlock Teams</h2>
        </div>
        <p class="text-sm text-muted">{status.identity.email} on {status.identity.server_url}</p>
        <input
          class="input"
          type="password"
          bind:value={unlockPass}
          placeholder="Teams passphrase"
          autocomplete="current-password"
          onkeydown={(e) => e.key === "Enter" && unlockPass && void unlock()}
        />
        <div>
          <button type="button" class="btn btn-primary" onclick={unlock} disabled={busy !== null || !unlockPass}>
            {#if busy === "unlock"}<span class="spinner size-3"></span>{/if}
            Unlock
          </button>
        </div>
      </div>
    {:else}
      <div class="flex flex-col gap-3">
        <!-- Identity -->
        <div class="panel flex flex-wrap items-center justify-between gap-3 p-4">
          <div class="flex min-w-0 flex-col">
            <span class="font-medium">{status.identity.email}</span>
            <span class="truncate text-xs text-muted">{status.identity.server_url}</span>
          </div>
          {#if status.fingerprint}
            <div class="flex items-center gap-2">
              <span class="text-xs text-muted">This device</span>
              <code class="text-xs">{status.fingerprint}</code>
              <button type="button" class="btn btn-ghost btn-sm" onclick={() => copy(status!.fingerprint!, "me")} title="Copy fingerprint">
                {#if copied === "me"}<Check size={14} />{:else}<Copy size={14} />{/if}
              </button>
            </div>
          {/if}
        </div>

        {#if joined}
          <div class="panel flex flex-col gap-2 border-primary p-4">
            <span class="font-medium">Joined {joined.name}</span>
            <span class="text-sm text-muted">
              Send this device's fingerprint to the admin so they can grant you the team key, then sync.
            </span>
            <div class="flex items-center gap-2">
              <code class="text-xs">{joined.my_fingerprint}</code>
              <button type="button" class="btn btn-ghost btn-sm" onclick={() => copy(joined!.my_fingerprint, "joined")}>
                {#if copied === "joined"}<Check size={14} />{:else}<Copy size={14} />{/if}
              </button>
              <button type="button" class="btn btn-ghost btn-sm" onclick={() => (joined = null)}>Dismiss</button>
            </div>
          </div>
        {/if}

        <!-- Teams -->
        {#each status.teams as team (team.team_id)}
          {@const isOpen = expanded === team.team_id}
          {@const teamConnections = connections.filter((c) => c.source_id === team.source_id)}
          <div class="panel flex flex-col">
            <div class="flex flex-wrap items-center justify-between gap-2 p-4">
              <button type="button" class="flex min-w-0 items-center gap-2 text-left" onclick={() => toggle(team)} aria-expanded={isOpen}>
                {#if isOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
                <span class="font-medium">{team.name}</span>
                <span class="badge badge-neutral">{team.hosts} {team.hosts === 1 ? "host" : "hosts"}</span>
                <span class="text-xs text-muted">
                  {team.last_sync_at ? `synced ${new Date(team.last_sync_at).toLocaleString()}` : "never synced"}
                </span>
              </button>
              <button type="button" class="btn btn-secondary btn-sm" onclick={() => syncTeam(team)} disabled={busy !== null}>
                <RefreshCw size={14} class={busy === `sync:${team.team_id}` ? "animate-spin" : ""} />
                Sync
              </button>
            </div>
            {#if lastSync[team.team_id]}
              <div class="px-4 pb-3 text-xs text-muted">{summarize(lastSync[team.team_id])}</div>
            {/if}

            {#if isOpen}
              <div class="flex flex-col gap-4 border-t border-base-300 p-4">
                <!-- Share -->
                <section class="flex flex-col gap-2">
                  <h3 class="flex items-center gap-1.5 text-sm font-semibold"><Share2 size={14} /> Share a connection</h3>
                  <div class="flex flex-wrap items-center gap-2">
                    <select class="input w-64" bind:value={shareChoice[team.team_id]}>
                      <option value="">Choose a connection…</option>
                      {#each localConnections as c (c.id)}
                        <option value={c.name}>{c.name} ({c.user}@{c.host})</option>
                      {/each}
                    </select>
                    <button type="button" class="btn btn-primary btn-sm" onclick={() => share(team)} disabled={busy !== null || !shareChoice[team.team_id]}>
                      Share
                    </button>
                  </div>
                  <p class="text-xs text-muted">Host, user, port, bastion, Kerberos and tags are shared; key paths and passwords never are.</p>
                </section>

                <!-- Team connections -->
                <section class="flex flex-col gap-1.5">
                  <h3 class="text-sm font-semibold">Team connections</h3>
                  {#each teamConnections as c (c.id)}
                    <div class="flex items-center justify-between gap-2 text-sm">
                      <span class="truncate">{c.name} <span class="text-muted">{c.user}@{c.host}:{c.port}</span></span>
                      <button type="button" class="btn btn-ghost btn-sm" onclick={() => unshare(team, c)} disabled={busy !== null}>
                        <Trash2 size={14} />
                        {confirmUnshare === c.id ? "Remove for everyone?" : "Unshare"}
                      </button>
                    </div>
                  {:else}
                    <span class="text-xs text-muted">None yet. Sync to fetch them, or share one above.</span>
                  {/each}
                </section>

                <!-- Members -->
                <section class="flex flex-col gap-1.5">
                  <h3 class="flex items-center gap-1.5 text-sm font-semibold"><Users size={14} /> Members</h3>
                  {#each members[team.team_id] ?? [] as m (m.email)}
                    <div class="flex flex-col gap-0.5 text-sm">
                      <div class="flex items-center justify-between gap-2">
                        <span>{m.email} <span class="badge {m.role === 'admin' ? 'badge-success' : 'badge-neutral'}">{m.role}</span></span>
                        {#if m.email !== status.identity.email}
                          <button type="button" class="btn btn-ghost btn-sm" onclick={() => removeMember(team, m)} disabled={busy !== null}>
                            {confirmRemove === `${team.team_id}:${m.email}` ? "Remove and rotate the key?" : "Remove"}
                          </button>
                        {/if}
                      </div>
                      {#each m.devices as d (d.fingerprint)}
                        <span class="pl-3 text-xs text-muted">{d.name || "device"} · <code>{d.fingerprint}</code></span>
                      {/each}
                    </div>
                  {:else}
                    <span class="text-xs text-muted">{busy === `members:${team.team_id}` ? "Loading…" : "No members loaded."}</span>
                  {/each}
                </section>

                <!-- Invite -->
                <section class="flex flex-col gap-2">
                  <h3 class="flex items-center gap-1.5 text-sm font-semibold"><UserPlus size={14} /> Invite</h3>
                  <div class="flex flex-wrap items-center gap-2">
                    <input class="input w-64" type="email" placeholder="ana@example.com" bind:value={inviteEmail[team.team_id]} />
                    <label class="flex items-center gap-1.5 text-sm">
                      <input type="checkbox" bind:checked={inviteAdmin[team.team_id]} /> admin
                    </label>
                    <button type="button" class="btn btn-secondary btn-sm" onclick={() => invite(team)} disabled={busy !== null || !(inviteEmail[team.team_id] ?? "").trim()}>
                      Invite
                    </button>
                  </div>
                  {#if invites[team.team_id]}
                    {@const inv = invites[team.team_id]}
                    <div class="flex flex-col gap-1.5 rounded-md bg-base-200 p-3">
                      <span class="text-xs text-muted">
                        Send {inv.email} these commands over a channel you trust. The token is shown only once
                        and expires {new Date(inv.expires_at).toLocaleDateString()}.
                      </span>
                      <pre class="overflow-x-auto text-xs select-text">{inviteText(inv)}</pre>
                      <div>
                        <button type="button" class="btn btn-ghost btn-sm" onclick={() => copy(inviteText(inv), `invite:${team.team_id}`)}>
                          {#if copied === `invite:${team.team_id}`}<Check size={14} />{:else}<Copy size={14} />{/if}
                          Copy
                        </button>
                      </div>
                    </div>
                  {/if}
                </section>

                <!-- Waiting devices -->
                <section class="flex flex-col gap-2">
                  <div class="flex items-center justify-between gap-2">
                    <h3 class="flex items-center gap-1.5 text-sm font-semibold"><KeyRound size={14} /> Devices waiting for the key</h3>
                    <button type="button" class="btn btn-ghost btn-sm" onclick={() => loadPending(team)} disabled={busy !== null}>
                      <RefreshCw size={14} class={busy === `pending:${team.team_id}` ? "animate-spin" : ""} />
                      Check
                    </button>
                  </div>
                  {#each pending[team.team_id] ?? [] as p (p.device.fingerprint)}
                    {@const typed = (grantInput[p.device.fingerprint] ?? "").trim()}
                    <div class="flex flex-col gap-1.5 text-sm">
                      <span>{p.email} · {p.device.name || "device"} · <code class="text-xs">{p.device.fingerprint}</code></span>
                      <div class="flex flex-wrap items-center gap-2">
                        <input
                          class="input input-mono w-96 text-xs"
                          placeholder="Paste the fingerprint {p.email} sent you"
                          bind:value={grantInput[p.device.fingerprint]}
                        />
                        <button
                          type="button"
                          class="btn btn-primary btn-sm"
                          onclick={() => grant(team, p)}
                          disabled={busy !== null || typed !== p.device.fingerprint}
                          title="Enabled only when the fingerprint you received matches this device"
                        >
                          Grant key
                        </button>
                        {#if typed && typed !== p.device.fingerprint}
                          <span class="text-xs text-error">Doesn't match this device. Don't grant it.</span>
                        {/if}
                      </div>
                    </div>
                  {:else}
                    <span class="text-xs text-muted">
                      {pending[team.team_id] ? "No device is waiting." : "Admins: check here after someone joins."}
                    </span>
                  {/each}
                </section>
              </div>
            {/if}
          </div>
        {/each}

        <!-- Create and join -->
        <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
          <div class="panel flex flex-col gap-2 p-4">
            <h3 class="flex items-center gap-1.5 text-sm font-semibold"><Plus size={14} /> Create a team</h3>
            <div class="flex gap-2">
              <input
                class="input flex-1"
                bind:value={newTeam}
                placeholder="platform"
                onkeydown={(e) => e.key === "Enter" && newTeam.trim() && void createTeam()}
              />
              <button type="button" class="btn btn-primary btn-sm" onclick={createTeam} disabled={busy !== null || !newTeam.trim()}>
                {#if busy === "create"}<span class="spinner size-3"></span>{/if}
                Create
              </button>
            </div>
          </div>
          <div class="panel flex flex-col gap-2 p-4">
            <h3 class="flex items-center gap-1.5 text-sm font-semibold"><LogIn size={14} /> Join a team</h3>
            <input class="input input-mono text-xs" bind:value={joinToken} placeholder="Invite token" />
            <input class="input input-mono text-xs" bind:value={joinFingerprint} placeholder="Admin fingerprint (SHA256:…)" />
            <details class="text-xs">
              <summary class="cursor-pointer text-muted">Finishing a join whose fingerprint didn't match?</summary>
              <input class="input input-mono mt-1.5 w-full text-xs" bind:value={joinTeamId} placeholder="Team id from the error message" />
            </details>
            <div>
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                onclick={join}
                disabled={busy !== null || !joinFingerprint.trim() || !(joinToken.trim() || joinTeamId.trim())}
              >
                {#if busy === "join"}<span class="spinner size-3"></span>{/if}
                Join
              </button>
            </div>
          </div>
        </div>
      </div>
    {/if}
  </div>
</div>
