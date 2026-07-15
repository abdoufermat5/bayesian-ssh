<script lang="ts">
  import {
    AppWindow,
    GripVertical,
    Layers,
    Link2,
    OctagonX,
    Play,
    Server,
    TerminalSquare,
    Unlink,
    X,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { tabPopOutDrag } from "$lib/actions/tabPopOutDrag";
  import {
    encodeSessionDrag,
    SESSION_DRAG_MIME,
    type SessionDragPayload,
    tabBarReattachDrop,
  } from "$lib/actions/tabBarReattachDrop";
  import {
    connectSSH,
    detachTab,
    disconnectTab,
    dockPopoutSession,
    getTerminalState,
    popOutTab,
    reattachSession,
  } from "$lib/stores/terminal.svelte";

  interface Props {
    connections: Connection[];
    searchQuery: string;
    onSearchInput: () => void;
    onCloseAll: () => void;
    onManageSessions: () => void;
  }

  let { connections, searchQuery = $bindable(), onSearchInput, onCloseAll, onManageSessions }: Props =
    $props();

  const terminalState = getTerminalState();

  const awaySessions = $derived.by((): SessionDragPayload[] => [
    ...terminalState.popoutSessions.map((session) => ({
      sessionId: session.id,
      kind: "popout" as const,
      name: session.name,
    })),
    ...terminalState.detachedSessions.map((session) => ({
      sessionId: session.id,
      kind: "detached" as const,
      name: session.name,
    })),
  ]);

  async function handleConnect(conn: Connection) {
    await connectSSH(conn);
  }

  async function handleDropReattach(payload: SessionDragPayload) {
    if (payload.kind === "popout") {
      await dockPopoutSession(payload.sessionId);
    } else {
      await reattachSession(payload.sessionId);
    }
  }

  function handleAwayDragStart(event: DragEvent, payload: SessionDragPayload) {
    event.dataTransfer?.setData(SESSION_DRAG_MIME, encodeSessionDrag(payload));
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
    }
    (event.target as HTMLElement).classList.add("away-chip-dragging");
  }

  function handleAwayDragEnd(event: DragEvent) {
    (event.target as HTMLElement).classList.remove("away-chip-dragging");
  }
</script>

<div class="flex flex-1 min-h-0 w-full overflow-hidden bg-surface">
  <aside class="w-60 min-w-[240px] border-r border-border bg-surface flex flex-col min-h-0">
    <div class="p-3.5 pb-2.5 border-b border-border flex flex-col gap-2 shrink-0">
      <span class="text-[10px] font-semibold tracking-wider text-muted uppercase pl-1">QUICK CONNECT</span>
      <input
        type="text"
        placeholder="Search hosts..."
        bind:value={searchQuery}
        oninput={onSearchInput}
        class="bg-surface-input border border-border text-primary py-1.5 px-2.5 rounded-lg outline-none text-xs transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] w-full"
      />
    </div>

    <div class="flex-1 min-h-0 overflow-y-auto overflow-x-hidden p-2 flex flex-col gap-1 overscroll-contain">
      {#if connections.length === 0}
        <p class="p-6 text-center text-muted text-[11px] m-0">No hosts found.</p>
      {:else}
        {#each connections as conn}
          <button
            class="w-full text-left p-2 border border-border rounded-lg bg-transparent cursor-pointer transition-all duration-100 flex items-center justify-between gap-2 hover:border-accent hover:bg-accent/4 group"
            onclick={() => handleConnect(conn)}
          >
            <div class="flex flex-col min-w-0">
              <span class="text-xs font-semibold text-primary overflow-hidden text-ellipsis whitespace-nowrap mb-0.5 group-hover:text-accent">{conn.name}</span>
              <span class="text-[10px] text-secondary font-mono overflow-hidden text-ellipsis whitespace-nowrap">{conn.user}@{conn.host}</span>
            </div>
            <span class="text-accent flex items-center opacity-70 shrink-0">
              <Play size={10} fill="currentColor" />
            </span>
          </button>
        {/each}
      {/if}
    </div>

    {#if terminalState.externalSessionCount > 0}
      <div class="p-2.5 pb-3 border-t border-border shrink-0">
        <button
          type="button"
          class="w-full inline-flex items-center justify-center gap-2 py-2 px-2.5 rounded-lg border border-accent/25 bg-accent/6 text-accent text-xs font-semibold cursor-pointer transition-colors duration-100 hover:bg-accent/12"
          onclick={onManageSessions}
        >
          <Layers size={14} />
          <span>{terminalState.externalSessionCount} away</span>
        </button>
      </div>
    {/if}
  </aside>

  <div class="flex-1 min-w-0 min-h-0 flex flex-col overflow-hidden bg-surface relative">
    {#if terminalState.tabs.length > 0 || terminalState.externalSessionCount > 0}
      <div
        class="flex items-end gap-2 px-3 pt-2 border-b border-border shrink-0 transition-colors duration-100"
        use:tabBarReattachDrop={handleDropReattach}
      >
        <div class="flex gap-0.5 pb-0 flex-1 min-w-0 overflow-x-auto">
          {#each terminalState.tabs as tab (tab.id)}
            <div
              class="bg-transparent border border-transparent border-b-none text-muted py-2 px-3 rounded-t-lg cursor-pointer text-xs font-medium flex items-center gap-1.5 transition-all duration-100 relative group
                {terminalState.activeTabId === tab.id
                  ? 'bg-white/5 text-primary !border-border !border-b-transparent after:absolute after:bottom-[-1px] after:left-0 after:right-0 after:h-[2px] after:bg-accent after:rounded-sm after:z-[2]'
                  : 'hover:bg-white/[0.03] hover:text-secondary'}"
              use:tabPopOutDrag={tab.id}
              onclick={() => (terminalState.activeTabId = tab.id)}
              role="button"
              tabindex="0"
              title="Drag tab out to open in a separate window"
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") terminalState.activeTabId = tab.id;
              }}
            >
              <Server size={12} />
              <span>{tab.name}</span>
              <button
                class="bg-transparent border-none text-muted cursor-pointer p-0.25 rounded flex items-center hover:bg-white/8 hover:text-primary opacity-0 group-hover:opacity-100 transition-opacity"
                title="Pop out to separate window (session keeps running)"
                onclick={(e) => {
                  e.stopPropagation();
                  void popOutTab(tab.id);
                }}
              >
                <AppWindow size={11} />
              </button>
              <button
                class="bg-transparent border-none text-muted cursor-pointer p-0.25 rounded flex items-center hover:bg-white/8 hover:text-accent transition-all opacity-0 group-hover:opacity-100 transition-opacity"
                title="Run in background (hide tab, program keeps running)"
                onclick={(e) => {
                  e.stopPropagation();
                  void detachTab(tab.id);
                }}
              >
                <Unlink size={11} />
              </button>
              <button
                class="bg-transparent border-none text-muted cursor-pointer p-0.5 rounded-full flex transition-all duration-100 hover:bg-danger/10 hover:text-danger"
                title="Terminate session"
                onclick={(e) => {
                  e.stopPropagation();
                  disconnectTab(tab.id);
                }}
              >
                <X size={12} />
              </button>
            </div>
          {/each}

          {#each awaySessions as session (session.sessionId)}
            <div
              class="away-session-chip inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-t-md border border-dashed border-accent/35 border-b-0 bg-accent/6 text-accent text-xs cursor-grab select-none max-w-[180px] hover:bg-accent/12"
              draggable="true"
              title="Drag to tab bar to reattach, or double-click"
              ondragstart={(event) => handleAwayDragStart(event, session)}
              ondragend={handleAwayDragEnd}
              ondblclick={() => void handleDropReattach(session)}
              role="button"
              tabindex="0"
            >
              <GripVertical size={11} />
              {#if session.kind === "popout"}
                <AppWindow size={12} />
              {:else}
                <Link2 size={12} />
              {/if}
              <span class="overflow-hidden text-ellipsis whitespace-nowrap">{session.name}</span>
            </div>
          {/each}
        </div>

        <div class="flex items-center gap-2.5 shrink-0">
          {#if terminalState.externalSessionCount > 0}
            <span class="text-[10px] text-muted whitespace-nowrap">Drop here to reattach</span>
          {/if}
          {#if terminalState.totalSessionCount > 0}
            <button
              type="button"
              class="inline-flex items-center gap-1.25 py-1.5 px-2.5 mb-1 border border-danger/25 rounded-lg bg-transparent text-red-300 text-[11px] font-semibold cursor-pointer shrink-0 transition-all duration-100 hover:bg-danger/8 hover:border-danger/40"
              onclick={onCloseAll}
              title="Terminate all active and detached SSH sessions"
            >
              <OctagonX size={14} />
              <span>Close all</span>
            </button>
          {/if}
        </div>
      </div>

      {#if terminalState.tabs.length > 0}
        <div class="flex-1 min-h-0 relative bg-surface-terminal border border-border border-t-0 rounded-b-lg overflow-hidden">
          {#each terminalState.tabs as tab (tab.id)}
            <div
              class="absolute inset-0 px-3 py-2 box-border overflow-hidden"
              class:hidden={terminalState.activeTabId !== tab.id}
            >
              <div id="terminal-{tab.id}" class="terminal-fit-target"></div>
            </div>
          {/each}
        </div>
      {:else}
        <div
          class="flex-1 flex flex-col items-center justify-center p-12 text-center min-h-0 border border-dashed border-transparent rounded-xl transition-all duration-200 reattach-drop-zone"
          use:tabBarReattachDrop={handleDropReattach}
        >
          <TerminalSquare size={48} class="text-muted mb-4" />
          <h3 class="text-base font-semibold mb-1.5 text-primary">Sessions running elsewhere</h3>
          <p class="text-xs text-muted m-0 max-w-[320px] leading-relaxed">
            Drag a session chip from below onto this area to reattach, or use the session manager.
          </p>
          <div class="flex flex-wrap gap-2 justify-center mt-4 mb-1">
            {#each awaySessions as session (session.sessionId)}
              <div
                class="away-session-chip inline-flex items-center gap-1.5 rounded-lg border border-dashed border-accent/35 border-b-0 py-2.5 px-3.5 max-w-[240px]"
                draggable="true"
                title="Drag to reattach"
                ondragstart={(event) => handleAwayDragStart(event, session)}
                ondragend={handleAwayDragEnd}
                role="button"
                tabindex="0"
              >
                <GripVertical size={12} />
                {#if session.kind === "popout"}
                  <AppWindow size={14} />
                {:else}
                  <Link2 size={14} />
                {/if}
                <span class="overflow-hidden text-ellipsis whitespace-nowrap">{session.name}</span>
              </div>
            {/each}
          </div>
          <button
            type="button"
            class="mt-4 inline-flex items-center gap-2 py-2 px-4 rounded-lg border border-accent/35 bg-accent/8 text-accent text-[13px] font-semibold cursor-pointer transition-colors duration-100 hover:bg-accent/14"
            onclick={onManageSessions}
          >
            <Layers size={14} />
            Manage sessions
          </button>
        </div>
      {/if}
    {:else}
      <div class="flex-1 flex flex-col items-center justify-center p-12 text-center min-h-0">
        <TerminalSquare size={48} class="text-muted mb-4" />
        <h3 class="text-base font-semibold mb-1.5 text-primary">No active sessions</h3>
        <p class="text-xs text-muted m-0 max-w-[320px] leading-relaxed">Select a host from Quick Connect to open an SSH session.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  :global(.tab-bar-drop-active) {
    background: rgba(59, 130, 246, 0.04) !important;
    box-shadow: inset 0 -2px 0 var(--color-accent) !important;
  }
  :global(.away-chip-dragging) {
    opacity: 0.55;
    cursor: grabbing;
  }
  :global(.tab-dragging-out) {
    opacity: 0.6;
    transform: translateY(-2px) scale(1.02);
    border-color: rgba(59, 130, 246, 0.3);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
    cursor: grabbing;
  }
</style>
