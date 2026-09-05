<script lang="ts">
  import {
    AppWindow,
    ArrowDown,
    ChevronDown,
    ChevronUp,
    Download,
    GripVertical,
    Layers,
    Link2,
    Minus,
    OctagonX,
    Play,
    Plus,
    Search,
    Server,
    TerminalSquare,
    Trash2,
    Unlink,
    X,
    ZoomIn,
    ZoomOut,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { downloadTerminalScrollback } from "$lib/utils/terminal-xterm";
  import { tabPopOutDrag } from "$lib/actions/tabPopOutDrag";
  import {
    encodeSessionDrag,
    SESSION_DRAG_MIME,
    type SessionDragPayload,
    tabBarReattachDrop,
  } from "$lib/actions/tabBarReattachDrop";
  import {
    closeTerminalSearch,
    connectSSH,
    detachTab,
    disconnectTab,
    dockPopoutSession,
    getTerminalState,
    popOutTab,
    reattachSession,
    toggleTerminalSearch,
    updateTerminalFontSize,
    getTerminalFontSize,
  } from "$lib/stores/terminal.svelte";

  interface Props {
    connections: Connection[];
    searchQuery?: string;
    onCloseAll: () => void;
    onManageSessions: () => void;
  }

  let { connections, searchQuery = $bindable(""), onCloseAll, onManageSessions }: Props = $props();

  const terminalState = getTerminalState();
  let tabSearchQueries = $state<Record<string, string>>({});
  let showQuickLauncher = $state(false);
  let launcherQuery = $state("");

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

  const filteredConnections = $derived.by(() => {
    const q = launcherQuery.trim().toLowerCase();
    if (!q) return connections.slice(0, 8);
    return connections.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        c.host.toLowerCase().includes(q) ||
        c.user.toLowerCase().includes(q),
    );
  });

  async function handleConnect(conn: Connection) {
    showQuickLauncher = false;
    launcherQuery = "";
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

  function zoomTerminal(delta: number) {
    const current = getTerminalFontSize();
    updateTerminalFontSize(current + delta);
  }
</script>

<div class="flex flex-1 min-h-0 w-full overflow-hidden bg-surface relative">
  <!-- Full-Width Terminal Workspace -->
  <div class="flex-1 min-w-0 min-h-0 flex flex-col overflow-hidden bg-surface relative">
    <!-- Tab Strip & Action Toolbar -->
    <div
      class="flex items-center justify-between gap-2 px-3 pt-2 border-b border-border/80 bg-surface-input/30 shrink-0 select-none transition-colors duration-100"
      use:tabBarReattachDrop={handleDropReattach}
    >
      <!-- Tabs scroll area -->
      <div class="flex items-end gap-1 pb-0 flex-1 min-w-0 overflow-x-auto scrollbar-none">
        {#each terminalState.tabs as tab (tab.id)}
          <div
            class="group relative flex items-center gap-2 px-3 py-2 rounded-t-lg cursor-pointer text-xs font-medium border transition-all duration-fast shrink-0
              {terminalState.activeTabId === tab.id
                ? 'bg-surface-terminal text-primary border-border border-b-transparent shadow-sm'
                : 'bg-surface-input/50 text-muted border-transparent hover:bg-surface-input hover:text-secondary'}"
            use:tabPopOutDrag={tab.id}
            onclick={() => (terminalState.activeTabId = tab.id)}
            role="button"
            tabindex="0"
            title="Active SSH tab (drag to pop out)"
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") terminalState.activeTabId = tab.id;
            }}
          >
            <span class="w-1.5 h-1.5 rounded-full bg-running shrink-0"></span>
            <span class="truncate max-w-[140px]">{tab.name}</span>

            <!-- Tab Actions on Hover -->
            <div class="flex items-center gap-0.5 ml-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <!-- Popout -->
              <button
                type="button"
                class="p-0.5 rounded text-muted hover:text-primary hover:bg-white/10 transition-colors border-none bg-transparent cursor-pointer"
                title="Pop out to separate window"
                onclick={(e) => {
                  e.stopPropagation();
                  void popOutTab(tab.id);
                }}
              >
                <AppWindow size={11} />
              </button>

              <!-- Detach / Run in background -->
              <button
                type="button"
                class="p-0.5 rounded text-muted hover:text-accent hover:bg-white/10 transition-colors border-none bg-transparent cursor-pointer"
                title="Run in background (detach)"
                onclick={(e) => {
                  e.stopPropagation();
                  void detachTab(tab.id);
                }}
              >
                <Unlink size={11} />
              </button>

              <!-- Close -->
              <button
                type="button"
                class="p-0.5 rounded text-muted hover:text-error hover:bg-error/15 transition-colors border-none bg-transparent cursor-pointer"
                title="Close session"
                onclick={(e) => {
                  e.stopPropagation();
                  disconnectTab(tab.id);
                }}
              >
                <X size={11} />
              </button>
            </div>
          </div>
        {/each}

        <!-- "+ New Session" Quick Launcher Button -->
        <div class="relative">
          <button
            type="button"
            class="flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-semibold text-muted hover:text-primary hover:bg-surface-hover transition-colors cursor-pointer border border-dashed border-border/70 mb-1"
            onclick={() => (showQuickLauncher = !showQuickLauncher)}
            title="Open new SSH session"
          >
            <Plus size={13} />
            <span>New Tab</span>
          </button>

          <!-- Quick Launcher Dropdown Popover -->
          {#if showQuickLauncher}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="fixed inset-0 z-40"
              onclick={() => (showQuickLauncher = false)}
              role="presentation"
            ></div>
            <div
              class="absolute left-0 top-full mt-1 w-72 rounded-xl border border-white/10 bg-surface-raised shadow-2xl p-2 z-50 flex flex-col gap-1.5 animate-in fade-in duration-100"
            >
              <div class="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-surface-input border border-border text-xs">
                <Search size={12} class="text-muted" />
                <input
                  type="text"
                  placeholder="Select host to connect..."
                  bind:value={launcherQuery}
                  class="bg-transparent border-none text-xs text-primary outline-none w-full placeholder:text-muted"
                />
              </div>

              <div class="max-h-48 overflow-y-auto flex flex-col gap-0.5 scrollbar-none">
                {#each filteredConnections as conn}
                  <button
                    type="button"
                    class="flex items-center justify-between px-2.5 py-2 rounded-lg text-left text-xs hover:bg-surface-hover transition-colors cursor-pointer border-none bg-transparent group"
                    onclick={() => handleConnect(conn)}
                  >
                    <div class="flex flex-col min-w-0">
                      <span class="font-semibold text-primary truncate group-hover:text-accent">{conn.name}</span>
                      <span class="font-mono text-[10px] text-muted truncate">{conn.user}@{conn.host}:{conn.port}</span>
                    </div>
                    <Play size={11} class="text-accent opacity-0 group-hover:opacity-100 transition-opacity shrink-0" fill="currentColor" />
                  </button>
                {:else}
                  <div class="p-3 text-center text-xs text-muted">No matching hosts</div>
                {/each}
              </div>
            </div>
          {/if}
        </div>

        <!-- Away Sessions Chips (Popout / Detached) -->
        {#each awaySessions as session (session.sessionId)}
          <div
            class="away-session-chip inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-t-md border border-dashed border-accent/35 border-b-0 bg-accent/10 text-accent text-xs cursor-grab select-none max-w-[160px] hover:bg-accent/20"
            draggable="true"
            title="Drag to tab bar to dock, or double-click"
            ondragstart={(event) => handleAwayDragStart(event, session)}
            ondragend={handleAwayDragEnd}
            ondblclick={() => void handleDropReattach(session)}
            role="button"
            tabindex="0"
          >
            <GripVertical size={11} class="opacity-70" />
            {#if session.kind === "popout"}
              <AppWindow size={11} />
            {:else}
              <Link2 size={11} />
            {/if}
            <span class="truncate">{session.name}</span>
          </div>
        {/each}
      </div>

      <!-- Right Action Toolbar inside Tab Strip -->
      <div class="flex items-center gap-1.5 shrink-0 pb-1">
        {#if terminalState.activeTabId}
          {@const activeTab = terminalState.tabs.find((t) => t.id === terminalState.activeTabId)}

          <!-- Find in terminal -->
          <button
            type="button"
            class="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium text-secondary hover:text-primary hover:bg-surface-hover transition-colors border border-border bg-surface-input/40 cursor-pointer"
            onclick={() => toggleTerminalSearch(terminalState.activeTabId ?? undefined)}
            title="Search Terminal (Ctrl+F)"
          >
            <Search size={12} />
            <span class="hidden md:inline">Find</span>
          </button>

          <!-- Export Log -->
          {#if activeTab?.term}
            <button
              type="button"
              class="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium text-secondary hover:text-primary hover:bg-surface-hover transition-colors border border-border bg-surface-input/40 cursor-pointer"
              onclick={() => downloadTerminalScrollback(activeTab.term!, activeTab.name)}
              title="Export session scrollback"
            >
              <Download size={12} />
              <span class="hidden md:inline">Export</span>
            </button>

            <!-- Clear Screen -->
            <button
              type="button"
              class="p-1 rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors border border-border bg-surface-input/40 cursor-pointer"
              onclick={() => activeTab.term?.clear()}
              title="Clear terminal screen"
            >
              <Trash2 size={12} />
            </button>

            <!-- Zoom controls -->
            <div class="flex items-center border border-border rounded-md bg-surface-input/40 p-0.5">
              <button
                type="button"
                class="p-1 text-secondary hover:text-primary hover:bg-surface-hover rounded cursor-pointer border-none bg-transparent"
                onclick={() => zoomTerminal(1)}
                title="Increase font size (Ctrl +)"
              >
                <Plus size={11} />
              </button>
              <button
                type="button"
                class="p-1 text-secondary hover:text-primary hover:bg-surface-hover rounded cursor-pointer border-none bg-transparent"
                onclick={() => zoomTerminal(-1)}
                title="Decrease font size (Ctrl -)"
              >
                <Minus size={11} />
              </button>
            </div>
          {/if}
        {/if}

        <!-- Away Sessions Badge -->
        {#if terminalState.externalSessionCount > 0}
          <button
            type="button"
            class="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-accent/40 bg-accent/15 text-accent text-[11px] font-semibold cursor-pointer hover:bg-accent/25 transition-colors"
            onclick={onManageSessions}
            title="Manage detached and popout sessions"
          >
            <Layers size={12} />
            <span>{terminalState.externalSessionCount} away</span>
          </button>
        {/if}

        <!-- Close All Sessions -->
        {#if terminalState.totalSessionCount > 0}
          <button
            type="button"
            class="flex items-center gap-1 px-2.5 py-1 rounded-md border border-error/30 text-error hover:bg-error/15 transition-colors text-[11px] font-semibold cursor-pointer bg-transparent"
            onclick={onCloseAll}
            title="Terminate all active SSH sessions"
          >
            <OctagonX size={12} />
            <span class="hidden sm:inline">Close all</span>
          </button>
        {/if}
      </div>
    </div>

    <!-- Active Terminal Surface or Empty State -->
    {#if terminalState.tabs.length > 0}
      <div class="flex-1 min-h-0 relative bg-surface-terminal overflow-hidden">
        {#each terminalState.tabs as tab (tab.id)}
          <div
            class="absolute inset-0 px-2 py-1 box-border overflow-hidden"
            class:hidden={terminalState.activeTabId !== tab.id}
          >
            <!-- In-Terminal Search Overlay -->
            {#if tab.showSearch}
              <div
                class="absolute top-3 right-5 z-40 bg-surface-raised/95 backdrop-blur-md border border-white/10 rounded-xl px-3 py-1.5 shadow-2xl flex items-center gap-2 text-xs animate-in fade-in"
              >
                <Search size={14} class="text-accent shrink-0" />
                <input
                  type="text"
                  placeholder="Find in scrollback..."
                  class="bg-transparent border-none text-primary outline-none text-xs w-48 font-mono"
                  bind:value={tabSearchQueries[tab.id]}
                  oninput={() => tab.searchAddon?.findNext(tabSearchQueries[tab.id] ?? "")}
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      if (e.shiftKey) tab.searchAddon?.findPrevious(tabSearchQueries[tab.id] ?? "");
                      else tab.searchAddon?.findNext(tabSearchQueries[tab.id] ?? "");
                    } else if (e.key === "Escape") {
                      closeTerminalSearch(tab.id);
                      tab.term?.focus();
                    }
                  }}
                />
                <button
                  type="button"
                  class="p-1 text-muted hover:text-primary rounded cursor-pointer border-none bg-transparent"
                  title="Previous match (Shift+Enter)"
                  onclick={() => tab.searchAddon?.findPrevious(tabSearchQueries[tab.id] ?? "")}
                >
                  <ChevronUp size={14} />
                </button>
                <button
                  type="button"
                  class="p-1 text-muted hover:text-primary rounded cursor-pointer border-none bg-transparent"
                  title="Next match (Enter)"
                  onclick={() => tab.searchAddon?.findNext(tabSearchQueries[tab.id] ?? "")}
                >
                  <ChevronDown size={14} />
                </button>
                <button
                  type="button"
                  class="p-1 text-muted hover:text-primary rounded cursor-pointer border-none bg-transparent"
                  title="Close search (Esc)"
                  onclick={() => {
                    closeTerminalSearch(tab.id);
                    tab.term?.focus();
                  }}
                >
                  <X size={14} />
                </button>
              </div>
            {/if}

            <!-- Xterm container -->
            <div id="terminal-{tab.id}" class="terminal-fit-target w-full h-full"></div>
          </div>
        {/each}
      </div>
    {:else if terminalState.externalSessionCount > 0}
      <!-- Away Sessions Reattach Landing Zone -->
      <div
        class="flex-1 flex flex-col items-center justify-center p-12 text-center min-h-0 reattach-drop-zone"
        use:tabBarReattachDrop={handleDropReattach}
      >
        <div class="w-14 h-14 rounded-2xl bg-accent/10 border border-accent/25 flex items-center justify-center text-accent mb-4">
          <Layers size={28} />
        </div>
        <h3 class="text-base font-bold text-primary mb-1">
          {terminalState.externalSessionCount} Background {terminalState.externalSessionCount === 1 ? 'Session' : 'Sessions'} Running
        </h3>
        <p class="text-xs text-muted max-w-sm leading-relaxed mb-4">
          Your remote SSH sessions are running in background processes or popout windows.
          Drag any session chip onto this canvas or click manage sessions.
        </p>
        <div class="flex flex-wrap gap-2 justify-center max-w-md mb-4">
          {#each awaySessions as session (session.sessionId)}
            <button
              type="button"
              class="away-session-chip inline-flex items-center gap-2 py-2 px-3.5 rounded-xl border border-dashed border-accent/40 bg-accent/10 text-accent text-xs font-semibold cursor-pointer hover:bg-accent/20 transition-all"
              onclick={() => void handleDropReattach(session)}
              title="Click to reattach session"
            >
              {#if session.kind === "popout"}
                <AppWindow size={13} />
              {:else}
                <Link2 size={13} />
              {/if}
              <span>{session.name}</span>
            </button>
          {/each}
        </div>
        <button
          type="button"
          class="btn btn-secondary"
          onclick={onManageSessions}
        >
          Manage All Sessions
        </button>
      </div>
    {:else}
      <!-- Empty State: No Active Sessions -->
      <div class="flex-1 flex flex-col items-center justify-center p-12 text-center min-h-0 bg-surface select-none">
        <div class="w-16 h-16 rounded-2xl bg-accent/10 border border-accent/20 flex items-center justify-center text-accent mb-4 shadow-sm">
          <TerminalSquare size={32} />
        </div>
        <h3 class="text-base font-bold text-primary mb-1.5">No Active Terminal Sessions</h3>
        <p class="text-xs text-muted max-w-md leading-relaxed mb-6">
          Choose a server from your Bayesian ranking to launch a high-performance interactive SSH terminal session.
        </p>

        <!-- Quick Connect Chips -->
        {#if connections.length > 0}
          <div class="flex flex-col items-center gap-2 max-w-lg w-full">
            <span class="eyebrow text-[10px]">Frequent Servers</span>
            <div class="flex flex-wrap gap-2 justify-center">
              {#each connections.slice(0, 4) as conn}
                <button
                  type="button"
                  class="flex items-center gap-2 px-3 py-2 rounded-xl border border-border bg-surface-input/60 hover:bg-surface-input hover:border-accent/40 hover:text-primary text-secondary text-xs font-semibold transition-all cursor-pointer group"
                  onclick={() => handleConnect(conn)}
                >
                  <Play size={10} class="text-accent" fill="currentColor" />
                  <span>{conn.name}</span>
                  <span class="text-[10px] font-mono text-muted group-hover:text-secondary">
                    {conn.user}@{conn.host}
                  </span>
                </button>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  :global(.tab-bar-drop-active) {
    background: rgba(59, 130, 246, 0.08) !important;
    box-shadow: inset 0 -2px 0 var(--color-accent) !important;
  }
  :global(.away-chip-dragging) {
    opacity: 0.5;
    cursor: grabbing;
  }
</style>
