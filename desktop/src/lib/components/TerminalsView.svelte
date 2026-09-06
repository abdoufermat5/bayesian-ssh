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
    focusPopoutSession,
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

<div class="flex flex-1 min-h-0 w-full overflow-hidden bg-surface-terminal relative">
  <div class="flex-1 min-w-0 min-h-0 flex flex-col overflow-visible bg-surface-terminal relative">
    <div
      class="terminal-command-deck shrink-0 select-none"
      use:tabBarReattachDrop={handleDropReattach}
    >
      <div class="terminal-tab-row">
        <div class="terminal-tab-strip">
          {#each terminalState.tabs as tab (tab.id)}
            <div
              class="terminal-tab group {terminalState.activeTabId === tab.id ? 'terminal-tab-active' : 'terminal-tab-idle'}"
              use:tabPopOutDrag={tab.id}
              onclick={() => (terminalState.activeTabId = tab.id)}
              role="button"
              tabindex="0"
              title={`${tab.name} (drag to pop out)`}
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") terminalState.activeTabId = tab.id;
              }}
            >
              <span class="terminal-live-rail"></span>
              <Server size={12} class="shrink-0 opacity-80" />
              <span class="terminal-tab-label" title={tab.name}>{tab.name}</span>

              <div class="terminal-tab-secondary-actions">
                <button
                  type="button"
                  class="terminal-tab-action"
                  title="Pop out to separate window"
                  onclick={(e) => {
                    e.stopPropagation();
                    void popOutTab(tab.id);
                  }}
                >
                  <AppWindow size={12} />
                </button>

                <button
                  type="button"
                  class="terminal-tab-action"
                  title="Run in background (detach)"
                  onclick={(e) => {
                    e.stopPropagation();
                    void detachTab(tab.id);
                  }}
                >
                  <Unlink size={12} />
                </button>
              </div>

              <button
                type="button"
                class="terminal-tab-close"
                title="Close session"
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
              class="away-session-chip terminal-away-chip group"
              draggable="true"
              title={session.kind === 'popout' ? 'Click to focus window, or drag to dock' : 'Click or drag to reattach'}
              ondragstart={(event) => handleAwayDragStart(event, session)}
              ondragend={handleAwayDragEnd}
              onclick={() => {
                if (session.kind === 'popout') {
                  void focusPopoutSession(session.sessionId);
                } else {
                  void handleDropReattach(session);
                }
              }}
              ondblclick={() => void handleDropReattach(session)}
              role="button"
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  if (session.kind === 'popout') void focusPopoutSession(session.sessionId);
                  else void handleDropReattach(session);
                }
              }}
            >
              <GripVertical size={12} class="opacity-70 shrink-0" />
              {#if session.kind === "popout"}
                <AppWindow size={12} class="shrink-0 text-accent" />
              {:else}
                <Link2 size={12} class="shrink-0 text-amber-400" />
              {/if}
              <span class="truncate">{session.name}</span>
              <button
                type="button"
                class="ml-1 p-0.5 rounded hover:bg-surface-elevated text-muted hover:text-primary transition-colors opacity-75 group-hover:opacity-100"
                title={session.kind === 'popout' ? 'Dock to main window' : 'Reattach session'}
                onclick={(e) => {
                  e.stopPropagation();
                  void handleDropReattach(session);
                }}
              >
                <Link2 size={11} />
              </button>
            </div>
          {/each}
        </div>

        <div class="terminal-new-session">
          <button
            type="button"
            class="terminal-control terminal-control-primary"
            onclick={() => (showQuickLauncher = !showQuickLauncher)}
            title="Open new SSH session"
          >
            <Plus size={14} />
            <span>New</span>
          </button>

          {#if showQuickLauncher}
            <div
              class="fixed inset-0 z-40"
              onclick={() => (showQuickLauncher = false)}
              role="presentation"
            ></div>
            <div
              class="terminal-launcher-popover"
            >
              <div class="terminal-launcher-search">
                <Search size={12} class="text-muted" />
                <input
                  type="text"
                  placeholder="Select host to connect..."
                  bind:value={launcherQuery}
                  class="terminal-launcher-input"
                />
              </div>

              <div class="terminal-launcher-list">
                {#each filteredConnections as conn}
                  <button
                    type="button"
                    class="terminal-launcher-option group"
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
      </div>

      <div class="terminal-tool-row">
        {#if terminalState.activeTabId}
          {@const activeTab = terminalState.tabs.find((t) => t.id === terminalState.activeTabId)}

          <button
            type="button"
            class="terminal-control"
            onclick={() => toggleTerminalSearch(terminalState.activeTabId ?? undefined)}
            title="Search Terminal (Ctrl+F)"
          >
            <Search size={12} />
            <span class="hidden md:inline">Find</span>
          </button>

          {#if activeTab?.term}
            <button
              type="button"
              class="terminal-control"
              onclick={() => downloadTerminalScrollback(activeTab.term!, activeTab.name)}
              title="Export session scrollback"
            >
              <Download size={12} />
              <span class="hidden md:inline">Export</span>
            </button>

            <button
              type="button"
              class="terminal-icon-control"
              onclick={() => activeTab.term?.clear()}
              title="Clear terminal screen"
            >
              <Trash2 size={12} />
            </button>

            <div class="terminal-stepper" aria-label="Terminal font size">
              <button
                type="button"
                class="terminal-stepper-button"
                onclick={() => zoomTerminal(1)}
                title="Increase font size (Ctrl +)"
              >
                <Plus size={11} />
              </button>
              <button
                type="button"
                class="terminal-stepper-button"
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
            class="terminal-control terminal-control-accent"
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
            class="terminal-control terminal-control-danger"
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
            {#if tab.showSearch}
              <div
                class="terminal-search-overlay"
              >
                <Search size={14} class="text-accent shrink-0" />
                <input
                  type="text"
                  placeholder="Find in scrollback..."
                  class="terminal-search-input"
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
                  class="terminal-overlay-button"
                  title="Previous match (Shift+Enter)"
                  onclick={() => tab.searchAddon?.findPrevious(tabSearchQueries[tab.id] ?? "")}
                >
                  <ChevronUp size={14} />
                </button>
                <button
                  type="button"
                  class="terminal-overlay-button"
                  title="Next match (Enter)"
                  onclick={() => tab.searchAddon?.findNext(tabSearchQueries[tab.id] ?? "")}
                >
                  <ChevronDown size={14} />
                </button>
                <button
                  type="button"
                  class="terminal-overlay-button terminal-overlay-button-danger"
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
        <div class="terminal-empty-icon">
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
              class="away-session-chip terminal-reattach-chip"
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
        <div class="terminal-empty-icon">
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
                  class="terminal-quick-chip group"
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
  .terminal-command-deck {
    position: relative;
    z-index: 30;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    padding: 4px 8px;
    overflow: visible;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .terminal-tab-row {
    display: flex;
    min-width: 0;
    align-items: stretch;
    gap: 6px;
    overflow: visible;
  }

  .terminal-tab-strip {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: stretch;
    gap: 4px;
    overflow-x: auto;
    overflow-y: hidden;
    padding-bottom: 1px;
  }

  .terminal-tab {
    position: relative;
    display: inline-grid;
    grid-template-columns: 2px auto minmax(50px, 140px) auto auto;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    max-width: 240px;
    flex: 0 0 auto;
    padding: 0 4px 0 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-secondary);
    background: var(--color-surface-input);
    cursor: pointer;
    transition: background-color var(--transition-duration-fast), border-color var(--transition-duration-fast),
      color var(--transition-duration-fast);
  }

  .terminal-tab:hover,
  .terminal-tab:focus-visible,
  .terminal-tab:focus-within {
    color: var(--color-primary);
    border-color: var(--color-border-hover);
    background: var(--color-surface-hover);
  }

  .terminal-tab-active {
    color: var(--color-primary);
    border-color: var(--color-border-hover);
    background: var(--color-surface-terminal);
  }

  .terminal-tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
  }

  .terminal-live-rail {
    align-self: stretch;
    width: 2px;
    border-radius: var(--radius-xs) 0 0 var(--radius-xs);
    background: var(--color-success);
  }

  .terminal-tab-secondary-actions {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    opacity: 0;
    transition: opacity var(--transition-duration-fast);
  }

  .terminal-tab:hover .terminal-tab-secondary-actions,
  .terminal-tab:focus-within .terminal-tab-secondary-actions {
    opacity: 0.8;
  }

  .terminal-tab-action,
  .terminal-tab-close,
  .terminal-overlay-button,
  .terminal-stepper-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    cursor: pointer;
    transition: background-color var(--transition-duration-fast), color var(--transition-duration-fast);
  }

  .terminal-tab-action,
  .terminal-tab-close {
    width: 20px;
    height: 20px;
    border-radius: var(--radius-xs);
  }

  .terminal-tab-close {
    opacity: 0;
    transition: opacity var(--transition-duration-fast);
  }

  .terminal-tab:hover .terminal-tab-close,
  .terminal-tab:focus-within .terminal-tab-close {
    opacity: 0.8;
  }

  .terminal-tab-action:hover,
  .terminal-tab-action:focus-visible,
  .terminal-overlay-button:hover,
  .terminal-overlay-button:focus-visible,
  .terminal-stepper-button:hover,
  .terminal-stepper-button:focus-visible {
    color: var(--color-primary);
    background: var(--color-surface-hover);
  }

  .terminal-tab-close:hover,
  .terminal-tab-close:focus-visible,
  .terminal-overlay-button-danger:hover,
  .terminal-overlay-button-danger:focus-visible {
    color: var(--color-error);
    background: color-mix(in srgb, var(--color-error) 16%, transparent);
  }

  .terminal-away-chip,
  .terminal-reattach-chip,
  .terminal-quick-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-input);
    color: var(--color-secondary);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color var(--transition-duration-fast), border-color var(--transition-duration-fast),
      color var(--transition-duration-fast);
  }

  .terminal-away-chip {
    max-width: 170px;
    min-height: 28px;
    padding: 0 8px;
    cursor: grab;
  }

  .terminal-reattach-chip,
  .terminal-quick-chip {
    padding: 6px 10px;
  }

  .terminal-away-chip:hover,
  .terminal-reattach-chip:hover,
  .terminal-quick-chip:hover {
    border-color: var(--color-border-hover);
    background: var(--color-surface-hover);
    color: var(--color-primary);
  }

  .terminal-new-session {
    position: relative;
    z-index: 50;
    flex: 0 0 auto;
  }

  .terminal-tool-row {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 3px;
    min-width: max-content;
  }

  .terminal-control,
  .terminal-icon-control,
  .terminal-stepper {
    min-height: 28px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-input);
    color: var(--color-secondary);
  }

  .terminal-control,
  .terminal-icon-control {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    padding: 0 8px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color var(--transition-duration-fast), border-color var(--transition-duration-fast),
      color var(--transition-duration-fast);
  }

  .terminal-icon-control {
    width: 28px;
    padding: 0;
  }

  .terminal-control:hover,
  .terminal-control:focus-visible,
  .terminal-icon-control:hover,
  .terminal-icon-control:focus-visible {
    border-color: var(--color-border-hover);
    background: var(--color-surface-hover);
    color: var(--color-primary);
  }

  .terminal-control-primary,
  .terminal-control-accent {
    border-color: var(--color-border-hover);
    color: var(--color-primary);
  }

  .terminal-control-danger {
    border-color: color-mix(in srgb, var(--color-error) 24%, var(--color-border));
    color: var(--color-error);
  }

  .terminal-control-danger:hover,
  .terminal-control-danger:focus-visible {
    background: color-mix(in srgb, var(--color-error) 12%, var(--color-surface));
    color: var(--color-error);
  }

  .terminal-stepper {
    display: inline-flex;
    align-items: center;
    padding: 1px;
  }

  .terminal-stepper-button {
    width: 22px;
    height: 22px;
    border-radius: var(--radius-xs);
  }

  .terminal-launcher-popover {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 60;
    display: flex;
    width: min(300px, calc(100vw - 24px));
    max-height: min(340px, calc(100vh - 96px));
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface-raised);
    padding: 6px;
    box-shadow: var(--shadow-lg);
    animation: popover-enter var(--transition-duration-base) var(--ease-out) forwards;
  }

  .terminal-launcher-search {
    display: flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-input);
    padding: 5px 8px;
    font-size: 11px;
  }

  .terminal-launcher-input,
  .terminal-search-input {
    width: 100%;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--color-primary);
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .terminal-launcher-input::placeholder,
  .terminal-search-input::placeholder {
    color: var(--color-muted);
  }

  .terminal-launcher-list {
    display: flex;
    max-height: 240px;
    flex-direction: column;
    gap: 1px;
    overflow-y: auto;
    padding-right: 2px;
  }

  .terminal-launcher-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    padding: 6px 8px;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--transition-duration-fast), border-color var(--transition-duration-fast);
  }

  .terminal-launcher-option:hover,
  .terminal-launcher-option:focus-visible {
    background: var(--color-surface-hover);
  }

  .terminal-search-overlay {
    position: absolute;
    top: 10px;
    right: 16px;
    z-index: 40;
    display: flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-raised);
    padding: 4px 6px;
    box-shadow: var(--shadow-lg);
    animation: popover-enter var(--transition-duration-base) var(--ease-out) forwards;
  }

  .terminal-search-input {
    width: 200px;
  }

  .terminal-overlay-button {
    width: 24px;
    height: 24px;
    border-radius: var(--radius-xs);
  }

  .terminal-empty-icon {
    display: flex;
    width: 48px;
    height: 48px;
    align-items: center;
    justify-content: center;
    margin-bottom: 14px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface-input);
    color: var(--color-muted);
  }

  :global(.tab-bar-drop-active) {
    background: color-mix(in srgb, var(--color-accent) 10%, var(--color-surface)) !important;
    box-shadow: inset 0 -2px 0 var(--color-accent) !important;
  }
  :global(.away-chip-dragging) {
    opacity: 0.5;
    cursor: grabbing;
  }
</style>
