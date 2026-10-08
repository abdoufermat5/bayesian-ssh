<script lang="ts">
  import {
    AppWindow,
    ChevronDown,
    ChevronUp,
    Download,
    Eraser,
    Layers,
    Link2,
    Minus,
    MoreHorizontal,
    OctagonX,
    Plus,
    Search,
    TerminalSquare,
    Unlink,
    X,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { formatRelative } from "$lib/utils/timezone";
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
    detachTab,
    disconnectTab,
    dockPopoutSession,
    focusPopoutSession,
    getTerminalFontSize,
    getTerminalState,
    popOutTab,
    reattachSession,
    toggleTerminalSearch,
    updateTerminalFontSize,
    type TerminalTab,
    type TerminalTabStatus,
  } from "$lib/stores/terminal.svelte";

  interface Props {
    connections: Connection[];
    searchQuery?: string;
    onCloseAll: () => void;
    onManageSessions: () => void;
    onConnect: (conn: Connection) => void | Promise<void>;
  }

  let {
    connections,
    searchQuery = $bindable(""),
    onCloseAll,
    onManageSessions,
    onConnect,
  }: Props = $props();

  const terminalState = getTerminalState();
  const activeTab = $derived(terminalState.activeTab);

  let tabSearchQueries = $state<Record<string, string>>({});
  let showLauncher = $state(false);
  let launcherQuery = $state("");
  let launcherIndex = $state(0);
  let showMoreMenu = $state(false);
  let tabStrip = $state<HTMLDivElement | null>(null);

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

  /** Most recently used hosts first; never-used hosts keep their ranking. */
  const recentHosts = $derived.by(() => {
    const used = connections
      .filter((c) => c.last_used)
      .sort((a, b) => Date.parse(b.last_used!) - Date.parse(a.last_used!));
    const rest = connections.filter((c) => !c.last_used);
    return [...used, ...rest].slice(0, 6);
  });

  const launcherResults = $derived.by(() => {
    const q = launcherQuery.trim().toLowerCase();
    if (!q) return connections.slice(0, 8);
    return connections
      .filter(
        (c) =>
          c.name.toLowerCase().includes(q) ||
          c.host.toLowerCase().includes(q) ||
          c.user.toLowerCase().includes(q),
      )
      .slice(0, 30);
  });

  // Keep the active tab visible in an overflowing strip.
  $effect(() => {
    const id = terminalState.activeTabId;
    if (!id || !tabStrip) return;
    tabStrip
      .querySelector<HTMLElement>(`[data-tab-id="${id}"]`)
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });

  function statusDot(status: TerminalTabStatus): string {
    switch (status) {
      case "connected":
        return "status-dot-success";
      case "connecting":
        return "status-dot-warning";
      case "error":
        return "status-dot-error";
      default:
        return "status-dot-offline";
    }
  }

  function statusLabel(status: TerminalTabStatus): string {
    switch (status) {
      case "connected":
        return "Connected";
      case "connecting":
        return "Connecting";
      case "error":
        return "Failed to connect";
      default:
        return "Disconnected";
    }
  }

  function hostLine(conn: Connection): string {
    return `${conn.user}@${conn.host}${conn.port !== 22 ? `:${conn.port}` : ""}`;
  }

  function openLauncher() {
    showMoreMenu = false;
    launcherQuery = "";
    launcherIndex = 0;
    showLauncher = !showLauncher;
  }

  function closeMenus() {
    showLauncher = false;
    showMoreMenu = false;
  }

  async function handleConnect(conn: Connection) {
    closeMenus();
    try {
      await onConnect(conn);
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  function handleLauncherKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      launcherIndex = Math.min(launcherIndex + 1, launcherResults.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      launcherIndex = Math.max(launcherIndex - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const conn = launcherResults[launcherIndex];
      if (conn) void handleConnect(conn);
    }
  }

  async function handleDropReattach(payload: SessionDragPayload) {
    try {
      if (payload.kind === "popout") {
        await dockPopoutSession(payload.sessionId);
      } else {
        await reattachSession(payload.sessionId);
      }
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  function activateAway(session: SessionDragPayload) {
    if (session.kind === "popout") void focusPopoutSession(session.sessionId);
    else void handleDropReattach(session);
  }

  function handleAwayDragStart(event: DragEvent, payload: SessionDragPayload) {
    event.dataTransfer?.setData(SESSION_DRAG_MIME, encodeSessionDrag(payload));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
    (event.currentTarget as HTMLElement).classList.add("away-chip-dragging");
  }

  function handleAwayDragEnd(event: DragEvent) {
    (event.currentTarget as HTMLElement).classList.remove("away-chip-dragging");
  }

  /** Vertical wheel scrolls the overflowing tab strip sideways. */
  function horizontalWheel(node: HTMLElement) {
    const onWheel = (e: WheelEvent) => {
      if (e.shiftKey || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      if (node.scrollWidth <= node.clientWidth) return;
      e.preventDefault();
      node.scrollLeft += e.deltaY;
    };
    node.addEventListener("wheel", onWheel, { passive: false });
    return { destroy: () => node.removeEventListener("wheel", onWheel) };
  }

  function zoomTerminal(delta: number) {
    updateTerminalFontSize(getTerminalFontSize() + delta);
  }

  function runSearch(tab: TerminalTab, backwards = false) {
    const query = tabSearchQueries[tab.id] ?? "";
    if (!query) return;
    if (backwards) tab.searchAddon?.findPrevious(query);
    else tab.searchAddon?.findNext(query);
  }

  function closeSearch(tab: TerminalTab) {
    tab.searchAddon?.clearDecorations();
    closeTerminalSearch(tab.id);
    tab.term?.focus();
  }

  function runMenuAction(action: () => void | Promise<void>) {
    showMoreMenu = false;
    void action();
  }

  function focusOnMount(node: HTMLInputElement) {
    requestAnimationFrame(() => {
      node.focus();
      node.select();
    });
  }

  function handleWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && (showLauncher || showMoreMenu)) {
      e.preventDefault();
      closeMenus();
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#snippet recentHostList()}
  <div class="mt-5 w-full max-w-md text-left">
    <div class="section-label mb-2 px-1">Recent hosts</div>
    <div class="panel overflow-hidden">
      {#each recentHosts as conn, i (conn.id)}
        <button
          type="button"
          class="group flex h-11 w-full cursor-pointer items-center gap-3 px-3 text-left transition-colors duration-fast hover:bg-surface-hover {i > 0 ? 'border-t border-border-subtle' : ''}"
          title="Connect to {conn.name}"
          onclick={() => handleConnect(conn)}
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

{#snippet awayIcon(kind: SessionDragPayload["kind"])}
  {#if kind === "popout"}
    <AppWindow size={14} class="shrink-0" />
  {:else}
    <Unlink size={14} class="shrink-0" />
  {/if}
{/snippet}

{#if terminalState.tabs.length > 0}
  <div class="flex min-h-0 w-full flex-1 flex-col overflow-hidden bg-surface-terminal">
    <!-- Keep a page heading for the sessions layout too: the tab strip acts as
         the visual header, so the accessible title is visually hidden. -->
    <h1 class="sr-only">Terminals</h1>
    <!-- Tab strip + active-session toolbar -->
    <div
      class="relative z-20 flex h-10 shrink-0 select-none items-center gap-1 border-b border-border bg-surface px-2"
      use:tabBarReattachDrop={handleDropReattach}
    >
      <div
        bind:this={tabStrip}
        class="scrollbar-none flex min-w-0 items-center gap-0.5 overflow-x-auto"
        role="tablist"
        aria-label="Terminal sessions"
        tabindex="-1"
        use:horizontalWheel
      >
        {#each terminalState.tabs as tab (tab.id)}
          {@const isActive = terminalState.activeTabId === tab.id}
          <div
            data-tab-id={tab.id}
            class="group flex h-7 max-w-48 shrink-0 cursor-pointer items-center gap-2 rounded-md pl-2.5 pr-1 text-xs transition-colors duration-fast
              {isActive ? 'bg-surface-active text-primary' : 'text-muted hover:bg-surface-hover hover:text-primary'}"
            use:tabPopOutDrag={tab.id}
            onclick={() => (terminalState.activeTabId = tab.id)}
            onauxclick={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                void disconnectTab(tab.id);
              }
            }}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                terminalState.activeTabId = tab.id;
              }
            }}
            role="tab"
            tabindex="0"
            aria-selected={isActive}
            title="{tab.name} · {statusLabel(tab.status)} — drag out to open in a window"
          >
            <span class="status-dot status-dot-sm {statusDot(tab.status)}" aria-hidden="true"></span>
            <span class="min-w-0 truncate font-medium">{tab.name}</span>
            <button
              type="button"
              class="inline-flex size-5 shrink-0 items-center justify-center rounded-sm text-muted transition-colors duration-fast hover:bg-surface-hover hover:text-primary focus-visible:opacity-100
                {isActive ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}"
              aria-label="Close {tab.name}"
              title="Close session"
              onclick={(e) => {
                e.stopPropagation();
                void disconnectTab(tab.id);
              }}
            >
              <X size={14} />
            </button>
          </div>
        {/each}

        {#if awaySessions.length > 0}
          <span class="mx-1 h-4 w-px shrink-0 bg-border" aria-hidden="true"></span>
          {#each awaySessions as session (session.sessionId)}
            <div
              class="group flex h-7 shrink-0 cursor-grab items-center gap-1.5 rounded-md border border-dashed border-border pl-2 pr-0.5 text-xs text-muted transition-colors duration-fast hover:border-border-hover hover:text-primary"
              draggable="true"
              title={session.kind === "popout"
                ? `${session.name} — in its own window. Click to focus, drag here to dock.`
                : `${session.name} — running in the background. Click to reattach.`}
              ondragstart={(event) => handleAwayDragStart(event, session)}
              ondragend={handleAwayDragEnd}
              onclick={() => activateAway(session)}
              ondblclick={() => void handleDropReattach(session)}
              onkeydown={(e) => {
                if (e.key === "Enter") activateAway(session);
              }}
              role="button"
              tabindex="0"
            >
              {@render awayIcon(session.kind)}
              <span class="max-w-32 truncate">{session.name}</span>
              <button
                type="button"
                class="btn-icon btn-icon-sm size-5"
                aria-label={session.kind === "popout" ? `Dock ${session.name}` : `Reattach ${session.name}`}
                title={session.kind === "popout" ? "Dock to this window" : "Reattach"}
                onclick={(e) => {
                  e.stopPropagation();
                  void handleDropReattach(session);
                }}
              >
                <Link2 size={14} />
              </button>
            </div>
          {/each}
        {/if}
      </div>

      <div class="relative shrink-0">
        <button
          type="button"
          class="btn-icon {showLauncher ? 'bg-surface-hover text-primary' : ''}"
          aria-label="New session"
          aria-expanded={showLauncher}
          title="New session"
          onclick={openLauncher}
        >
          <Plus size={14} />
        </button>
        {#if showLauncher}
          <div class="fixed inset-0 z-40" role="presentation" onclick={closeMenus}></div>
          <div class="popover absolute left-0 top-full mt-1 flex w-72 flex-col gap-1">
            <div class="search-box h-8">
              <Search size={14} class="shrink-0" />
              <input
                type="text"
                placeholder="Connect to host…"
                aria-label="Filter hosts"
                bind:value={launcherQuery}
                oninput={() => (launcherIndex = 0)}
                onkeydown={handleLauncherKeydown}
                use:focusOnMount
              />
            </div>
            <div class="flex max-h-72 flex-col overflow-y-auto" role="listbox" aria-label="Hosts">
              {#each launcherResults as conn, i (conn.id)}
                <button
                  type="button"
                  class="menu-item h-auto flex-col items-start gap-0 py-1.5 {i === launcherIndex ? 'menu-item-active' : ''}"
                  role="option"
                  aria-selected={i === launcherIndex}
                  onmouseenter={() => (launcherIndex = i)}
                  onclick={() => handleConnect(conn)}
                >
                  <span class="w-full truncate text-sm text-primary">{conn.name}</span>
                  <span class="mono w-full truncate text-muted">{hostLine(conn)}</span>
                </button>
              {:else}
                <p class="px-2 py-3 text-center text-xs text-muted">No matching hosts</p>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <div class="min-w-2 flex-1 self-stretch" aria-hidden="true"></div>

      {#if activeTab}
        <button
          type="button"
          class="btn-icon {activeTab.showSearch ? 'bg-surface-hover text-primary' : ''}"
          aria-label="Find in terminal"
          aria-pressed={activeTab.showSearch}
          title="Find (Ctrl+F)"
          onclick={() => toggleTerminalSearch(activeTab.id)}
        >
          <Search size={14} />
        </button>

        <div class="flex h-7 shrink-0 items-center rounded-md border border-border" role="group" aria-label="Font size">
          <button
            type="button"
            class="btn-icon btn-icon-sm"
            aria-label="Decrease font size"
            title="Smaller text (Ctrl −)"
            onclick={() => zoomTerminal(-1)}
          >
            <Minus size={14} />
          </button>
          <span class="w-6 text-center text-xs tabular-nums text-secondary" title="Font size">{getTerminalFontSize()}</span>
          <button
            type="button"
            class="btn-icon btn-icon-sm"
            aria-label="Increase font size"
            title="Larger text (Ctrl +)"
            onclick={() => zoomTerminal(1)}
          >
            <Plus size={14} />
          </button>
        </div>
      {/if}

      {#if terminalState.externalSessionCount > 0}
        <button
          type="button"
          class="btn btn-ghost btn-sm"
          title="Manage background and pop-out sessions"
          onclick={onManageSessions}
        >
          <Layers size={14} />
          {terminalState.externalSessionCount} away
        </button>
      {/if}

      <div class="relative shrink-0">
        <button
          type="button"
          class="btn-icon {showMoreMenu ? 'bg-surface-hover text-primary' : ''}"
          aria-label="More session actions"
          aria-haspopup="menu"
          aria-expanded={showMoreMenu}
          title="More"
          onclick={() => {
            showLauncher = false;
            showMoreMenu = !showMoreMenu;
          }}
        >
          <MoreHorizontal size={14} />
        </button>
        {#if showMoreMenu}
          <div class="fixed inset-0 z-40" role="presentation" onclick={closeMenus}></div>
          <div class="popover absolute right-0 top-full mt-1 w-56" role="menu">
            {#if activeTab}
              <button
                type="button"
                class="menu-item"
                role="menuitem"
                onclick={() => runMenuAction(() => popOutTab(activeTab.id))}
              >
                <AppWindow size={14} />
                Open in new window
              </button>
              <button
                type="button"
                class="menu-item"
                role="menuitem"
                onclick={() => runMenuAction(() => detachTab(activeTab.id))}
              >
                <Unlink size={14} />
                Run in background
              </button>
              <div class="menu-separator"></div>
              <button
                type="button"
                class="menu-item"
                role="menuitem"
                disabled={!activeTab.ready}
                onclick={() =>
                  runMenuAction(() => {
                    if (activeTab.term) downloadTerminalScrollback(activeTab.term, activeTab.name);
                  })}
              >
                <Download size={14} />
                Export scrollback
              </button>
              <button
                type="button"
                class="menu-item"
                role="menuitem"
                disabled={!activeTab.ready}
                onclick={() => runMenuAction(() => activeTab.term?.clear())}
              >
                <Eraser size={14} />
                Clear screen
              </button>
              <div class="menu-separator"></div>
            {/if}
            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={() => runMenuAction(onManageSessions)}
            >
              <Layers size={14} />
              Manage sessions…
            </button>
            {#if terminalState.totalSessionCount > 0}
              <button
                type="button"
                class="menu-item menu-item-danger"
                role="menuitem"
                onclick={() => runMenuAction(onCloseAll)}
              >
                <OctagonX size={14} />
                Close all sessions
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Terminal surfaces; inactive tabs stay mounted with display:none. -->
    <div class="relative min-h-0 flex-1 overflow-hidden bg-surface-terminal">
      {#each terminalState.tabs as tab (tab.id)}
        <div
          class="absolute inset-0 box-border overflow-hidden p-2"
          class:hidden={terminalState.activeTabId !== tab.id}
          role="tabpanel"
          aria-label={tab.name}
        >
          {#if tab.showSearch}
            <div class="popover absolute right-4 top-2 z-10 flex items-center gap-0.5">
              <Search size={14} class="ml-1.5 shrink-0 text-muted" />
              <input
                type="text"
                placeholder="Find in scrollback"
                aria-label="Find in scrollback"
                class="h-7 w-52 min-w-0 bg-transparent px-1.5 text-sm text-primary outline-none placeholder:text-muted"
                bind:value={tabSearchQueries[tab.id]}
                use:focusOnMount
                oninput={() => runSearch(tab)}
                onkeydown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    runSearch(tab, e.shiftKey);
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    closeSearch(tab);
                  }
                }}
              />
              <button
                type="button"
                class="btn-icon btn-icon-sm"
                aria-label="Previous match"
                title="Previous match (Shift+Enter)"
                onclick={() => runSearch(tab, true)}
              >
                <ChevronUp size={14} />
              </button>
              <button
                type="button"
                class="btn-icon btn-icon-sm"
                aria-label="Next match"
                title="Next match (Enter)"
                onclick={() => runSearch(tab)}
              >
                <ChevronDown size={14} />
              </button>
              <span class="mx-0.5 h-4 w-px bg-border" aria-hidden="true"></span>
              <button
                type="button"
                class="btn-icon btn-icon-sm"
                aria-label="Close search"
                title="Close (Esc)"
                onclick={() => closeSearch(tab)}
              >
                <X size={14} />
              </button>
            </div>
          {/if}

          <div id="terminal-{tab.id}" class="h-full w-full"></div>
        </div>
      {/each}
    </div>
  </div>
{:else}
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

  <div class="view-body flex flex-col" use:tabBarReattachDrop={handleDropReattach}>
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
              ondragstart={(event) => handleAwayDragStart(event, session)}
              ondragend={handleAwayDragEnd}
            >
              <span class="text-muted">{@render awayIcon(session.kind)}</span>
              <div class="flex min-w-0 flex-1 flex-col">
                <span class="truncate text-sm text-primary">{session.name}</span>
                <span class="text-xs text-muted">
                  {session.kind === "popout" ? "In its own window" : "Running in the background"}
                </span>
              </div>
              {#if session.kind === "popout"}
                <button
                  type="button"
                  class="btn btn-ghost btn-sm"
                  onclick={() => void focusPopoutSession(session.sessionId)}
                >
                  Focus
                </button>
              {/if}
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                onclick={() => void handleDropReattach(session)}
              >
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
          {recentHosts.length > 0
            ? "Pick a host to open an SSH session."
            : "Add a host in Hosts, then connect to it here."}
        </p>

        {#if recentHosts.length > 0}{@render recentHostList()}{/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  /* Drop target highlight while dragging a pop-out/background session. */
  :global(.tab-bar-drop-active) {
    background: color-mix(in srgb, var(--color-accent) 6%, var(--color-surface)) !important;
    outline: 1px dashed color-mix(in srgb, var(--color-accent) 60%, transparent);
    outline-offset: -3px;
  }
  /* Tab being dragged far enough to pop out on release. */
  :global(.tab-dragging-out) {
    opacity: 0.55;
    outline: 1px dashed var(--color-accent);
    outline-offset: -1px;
  }
  :global(.away-chip-dragging) {
    opacity: 0.5;
    cursor: grabbing;
  }
</style>
