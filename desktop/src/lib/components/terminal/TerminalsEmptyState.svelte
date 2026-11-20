<script lang="ts">
  import { AppWindow, Layers, Link2, TerminalSquare, Unlink } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { tabBarReattachDrop } from "$lib/actions/tabBarReattachDrop";
  import { focusPopoutSession } from "$lib/stores/terminal.svelte";
  import { formatRelative } from "$lib/utils/timezone";
  import { hostLine } from "$lib/utils/terminal-status";
  import { bringBackSession, endAwayDrag, listAwaySessions, startAwayDrag } from "./away-sessions";

  interface Props {
    connections: Connection[];
    onManageSessions: () => void;
    onConnect: (conn: Connection) => void;
  }

  let { connections, onManageSessions, onConnect }: Props = $props();

  const awaySessions = $derived(listAwaySessions());

  /** Most recently used hosts first; never-used hosts keep their ranking. */
  const recentHosts = $derived.by(() => {
    const used = connections
      .filter((c) => c.last_used)
      .sort((a, b) => Date.parse(b.last_used!) - Date.parse(a.last_used!));
    const rest = connections.filter((c) => !c.last_used);
    return [...used, ...rest].slice(0, 6);
  });
</script>

{#snippet recentHostList()}
  <div class="mt-5 w-full max-w-md text-left">
    <div class="section-label mb-2 px-1">Recent hosts</div>
    <div class="panel overflow-hidden">
      {#each recentHosts as conn, i (conn.id)}
        <button
          type="button"
          class="group flex h-11 w-full cursor-pointer items-center gap-3 px-3 text-left transition-colors duration-fast hover:bg-surface-hover {i > 0 ? 'border-t border-border-subtle' : ''}"
          title="Connect to {conn.name}"
          onclick={() => onConnect(conn)}
        >
          <TerminalSquare size={14} class="shrink-0 text-muted group-hover:text-primary" />
          <div class="flex min-w-0 flex-1 flex-col">
            <span class="truncate text-sm text-primary">{conn.name}</span>
            <span class="mono truncate text-muted">{hostLine(conn)}</span>
          </div>
          <span class="shrink-0 text-xs text-muted group-hover:hidden">
            {conn.last_used ? formatRelative(conn.last_used) : "Never used"}
          </span>
          <span class="hidden shrink-0 text-xs text-accent group-hover:inline">Connect</span>
        </button>
      {/each}
    </div>
  </div>
{/snippet}

<header class="view-header">
  <div class="flex min-w-0 items-baseline gap-2.5">
    <h1 class="view-title">Terminals</h1>
    <span class="text-sm tabular-nums text-muted">
      {awaySessions.length > 0 ? `${awaySessions.length} away` : "No open sessions"}
    </span>
  </div>
  {#if awaySessions.length > 0}
    <div class="view-actions">
      <button type="button" class="btn btn-ghost" onclick={onManageSessions}>
        <Layers size={14} />
        Manage sessions
      </button>
    </div>
  {/if}
</header>

<div class="view-body flex flex-col" use:tabBarReattachDrop={bringBackSession}>
  <div class="empty-state my-auto">
    {#if awaySessions.length > 0}
      <div class="empty-state-icon"><Layers size={16} /></div>
      <p class="empty-state-title">
        {awaySessions.length === 1 ? "1 session is" : `${awaySessions.length} sessions are`} running elsewhere
      </p>
      <p class="empty-state-desc">Reattach a session to bring it back into this window, or drag it here.</p>

      <div class="panel mt-5 w-full max-w-md overflow-hidden text-left" role="list">
        {#each awaySessions as session, i (session.sessionId)}
          <div
            class="flex h-11 items-center gap-3 px-3 {i > 0 ? 'border-t border-border-subtle' : ''}"
            draggable="true"
            role="listitem"
            ondragstart={(event) => startAwayDrag(event, session)}
            ondragend={endAwayDrag}
          >
            <span class="text-muted">
              {#if session.kind === "popout"}
                <AppWindow size={14} class="shrink-0" />
              {:else}
                <Unlink size={14} class="shrink-0" />
              {/if}
            </span>
            <div class="flex min-w-0 flex-1 flex-col">
              <span class="truncate text-sm text-primary">{session.name}</span>
              <span class="text-xs text-muted">
                {session.kind === "popout" ? "In its own window" : "Running in the background"}
              </span>
            </div>
            {#if session.kind === "popout"}
              <button type="button" class="btn btn-ghost btn-sm" onclick={() => void focusPopoutSession(session.sessionId)}>
                Focus
              </button>
            {/if}
            <button type="button" class="btn btn-secondary btn-sm" onclick={() => void bringBackSession(session)}>
              <Link2 size={14} />
              {session.kind === "popout" ? "Dock" : "Reattach"}
            </button>
          </div>
        {/each}
      </div>
      {#if recentHosts.length > 0}{@render recentHostList()}{/if}
    {:else}
      <div class="empty-state-icon"><TerminalSquare size={16} /></div>
      <p class="empty-state-title">No open sessions</p>
      <p class="empty-state-desc">
        {recentHosts.length > 0 ? "Pick a host to open an SSH session." : "Add a host in Hosts, then connect to it here."}
      </p>

      {#if recentHosts.length > 0}{@render recentHostList()}{/if}
    {/if}
  </div>
</div>
