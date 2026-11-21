<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import {
    AppWindow,
    Columns2,
    Download,
    Eraser,
    House,
    Layers,
    Link2,
    Minus,
    MoreHorizontal,
    OctagonX,
    Plus,
    Rows2,
    Search,
    Unlink,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { appState } from "$lib/stores/appState.svelte";
  import { downloadTerminalScrollback, terminalShortcutId } from "$lib/utils/terminal-xterm";
  import type { TabDragHandlers } from "$lib/actions/tabDrag";
  import { tabBarReattachDrop } from "$lib/actions/tabBarReattachDrop";
  import GroupMenu from "$lib/components/terminal/GroupMenu.svelte";
  import HostLauncher from "$lib/components/terminal/HostLauncher.svelte";
  import SplitDividers from "$lib/components/terminal/SplitDividers.svelte";
  import StripTab from "$lib/components/terminal/StripTab.svelte";
  import TabMenu from "$lib/components/terminal/TabMenu.svelte";
  import TerminalPane from "$lib/components/terminal/TerminalPane.svelte";
  import {
    activateAwaySession,
    bringBackSession,
    endAwayDrag,
    listAwaySessions,
    startAwayDrag,
  } from "$lib/components/terminal/away-sessions";
  import {
    cssPercent,
    dropZone,
    layoutBoxes,
    paneAt,
    paneIds,
    zoneRect,
    type Rect,
    type SplitSide,
  } from "$lib/utils/split-layout";
  import { tabGroupColorVar, tabGroupLabel, type TabGroup } from "$lib/utils/tab-groups";
  import { getTerminalFontSize, updateTerminalFontSize } from "$lib/stores/terminal-appearance.svelte";
  import {
    addTabToGroup,
    detachTab,
    getTerminalState,
    moveTab,
    openTabGroup,
    popOutTab,
    runTerminalShortcut,
    splitTab,
    toggleTabGroupCollapsed,
    toggleTerminalSearch,
    type ConnectOptions,
    type TerminalTab,
  } from "$lib/stores/terminal.svelte";

  interface Props {
    connections: Connection[];
    onCloseAll: () => void;
    onManageSessions: () => void;
    onConnect: (conn: Connection, options?: Pick<ConnectOptions, "split">) => void | Promise<void>;
    /** The host list (home) is the surface in view; terminals stay mounted. */
    homeActive: boolean;
    onGoHome: () => void;
    /** Console home surface: the host list, shared with the standalone view. */
    home: Snippet;
  }

  let {
    connections,
    onCloseAll,
    onManageSessions,
    onConnect,
    homeActive,
    onGoHome,
    home,
  }: Props = $props();

  const terminalState = getTerminalState();
  const activeTab = $derived(terminalState.activeTab);

  /**
   * The Console (host list or terminals) is the workspace in front. While
   * another view is open the whole Console is hidden; the host list must not
   * mount there, because its window-level shortcuts would stay live behind
   * that view.
   */
  const consoleInView = $derived(
    appState.activeTab === "connections" || appState.activeTab === "terminals",
  );
  const homeInView = $derived(homeActive && consoleInView);

  let showLauncher = $state(false);
  /** Side of the active tab a launcher connection opens on; null opens a plain tab. */
  let launcherSplit = $state<SplitSide | null>(null);
  /** Triggering button used to anchor HostLauncher with viewport clamping. */
  let launcherAnchor = $state<HTMLElement | null>(null);
  let newSessionBtn = $state<HTMLButtonElement | null>(null);
  let splitRightBtn = $state<HTMLButtonElement | null>(null);
  let splitDownBtn = $state<HTMLButtonElement | null>(null);
  let showMoreMenu = $state(false);
  let tabStrip = $state<HTMLDivElement | null>(null);
  let stripBar = $state<HTMLDivElement | null>(null);
  let surfaceArea = $state<HTMLDivElement | null>(null);
  let tabMenu = $state<{ tabId: string; x: number; y: number } | null>(null);
  let groupMenu = $state<{ groupId: string; x: number; y: number } | null>(null);
  /** Element to refocus when a tab or group menu closes. */
  let menuOpener: HTMLElement | null = null;

  type DropTarget =
    | { kind: "strip"; beforeTabId: string | null; markerX: number }
    | { kind: "group"; groupId: string }
    | { kind: "pane"; tabId: string; side: SplitSide | "center"; rect: Rect }
    | { kind: "away" };
  let drag = $state<{ tabId: string; target: DropTarget } | null>(null);

  const awaySessions = $derived(listAwaySessions());
  const groupsById = $derived(new Map(terminalState.groups.map((g) => [g.id, g])));

  type StripItem =
    | { kind: "tab"; tab: TerminalTab; group: TabGroup | undefined }
    | { kind: "group"; group: TabGroup; count: number };

  /** Tabs in order, each group's tabs preceded by its chip; collapsed groups show only the chip. */
  const stripItems = $derived.by((): StripItem[] => {
    const items: StripItem[] = [];
    let current: string | null = null;
    for (const tab of terminalState.tabs) {
      const group = tab.groupId ? groupsById.get(tab.groupId) : undefined;
      if (group && group.id !== current) {
        const count = terminalState.tabs.filter((t) => t.groupId === group.id).length;
        items.push({ kind: "group", group, count });
      }
      current = group?.id ?? null;
      if (!group?.collapsed) items.push({ kind: "tab", tab, group });
    }
    return items;
  });

  const splitLayout = $derived(terminalState.activeSplit ? layoutBoxes(terminalState.activeSplit) : null);
  const paneRects = $derived(new Map(splitLayout?.panes.map((p) => [p.tabId, p.rect]) ?? []));
  const splitTabIds = $derived(new Set(terminalState.splits.flatMap(paneIds)));

  // Keep the active tab visible in an overflowing strip.
  $effect(() => {
    const id = terminalState.activeTabId;
    if (!id || !tabStrip) return;
    tabStrip
      .querySelector<HTMLElement>(`[data-tab-id="${id}"]`)
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });

  // ---- Launcher and toolbar menu ------------------------------------------

  function openLauncher(split: SplitSide | null = null, anchor: HTMLElement | null = null) {
    showMoreMenu = false;
    const closing = showLauncher && launcherSplit === split;
    showLauncher = !closing;
    launcherSplit = split;
    launcherAnchor = closing
      ? null
      : (anchor ?? (split === "right" ? splitRightBtn : split === "bottom" ? splitDownBtn : newSessionBtn));
  }

  function closeMenus() {
    showLauncher = false;
    launcherAnchor = null;
    showMoreMenu = false;
  }

  async function handleConnect(conn: Connection) {
    const split = launcherSplit;
    closeMenus();
    try {
      await onConnect(conn, split ? { split } : undefined);
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  async function handleOpenGroup(groupId: string) {
    closeMenus();
    try {
      await openTabGroup(groupId, connections);
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  /** A session tab was chosen: leave the home surface for the terminal. */
  function handleSelectTab() {
    if (homeActive) appState.goToTerminals();
  }

  function runMenuAction(action: () => void | Promise<void>) {
    showMoreMenu = false;
    void action();
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

  function handleWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && (showLauncher || showMoreMenu)) {
      e.preventDefault();
      closeMenus();
      return;
    }
    // Focus inside the Terminals view but outside xterm (tab strip, failure
    // panel). xterm handles its own keys and stops them. Zoom is left to the
    // focused terminal.
    const target = e.target;
    if (!(target instanceof HTMLElement) || !target.closest("[data-terminals-view]")) return;
    // The host list owns the keyboard while it is the surface in view.
    if (homeActive) return;
    const id = terminalShortcutId(e);
    if (!id || id.startsWith("term-zoom")) return;
    if (id === "term-cancel" && target instanceof HTMLInputElement) return;
    if (runTerminalShortcut(id)) e.preventDefault();
  }

  // ---- Tab and group menus ------------------------------------------------

  /** Menu position: the pointer for a right-click, below the element for a key. */
  function menuPoint(e: MouseEvent | KeyboardEvent): { x: number; y: number } {
    if (e instanceof MouseEvent && e.type === "contextmenu") return { x: e.clientX, y: e.clientY };
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: rect.left, y: rect.bottom + 4 };
  }

  function openTabMenu(e: MouseEvent | KeyboardEvent, tabId: string) {
    e.preventDefault();
    closeMenus();
    groupMenu = null;
    menuOpener = e.currentTarget as HTMLElement;
    tabMenu = { tabId, ...menuPoint(e) };
  }

  function openGroupMenu(e: MouseEvent | KeyboardEvent, groupId: string) {
    e.preventDefault();
    closeMenus();
    tabMenu = null;
    menuOpener = e.currentTarget as HTMLElement;
    groupMenu = { groupId, ...menuPoint(e) };
  }

  function closeFloatingMenu() {
    tabMenu = null;
    groupMenu = null;
    menuOpener?.focus();
    menuOpener = null;
  }

  /** A group just started from the tab menu: open its editor where the menu was. */
  async function editNewGroup(groupId: string, at: { x: number; y: number }) {
    await tick();
    menuOpener = tabStrip?.querySelector<HTMLElement>(`[data-group-id="${groupId}"]`) ?? null;
    groupMenu = { groupId, ...at };
  }

  // ---- Dragging tabs ------------------------------------------------------

  function inside(rect: DOMRect, x: number, y: number): boolean {
    return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
  }

  function dropTargetAt(tabId: string, x: number, y: number): DropTarget {
    const bar = stripBar?.getBoundingClientRect();
    if (bar && inside(bar, x, y)) {
      const chip = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-group-id]");
      if (chip?.dataset.groupId) return { kind: "group", groupId: chip.dataset.groupId };
      const others = Array.from(tabStrip?.querySelectorAll<HTMLElement>("[data-tab-id]") ?? []).filter(
        (el) => el.dataset.tabId !== tabId,
      );
      const before = others.find((el) => {
        const r = el.getBoundingClientRect();
        return x < r.left + r.width / 2;
      });
      const edge = before
        ? before.getBoundingClientRect().left
        : (others.at(-1)?.getBoundingClientRect().right ?? bar.left);
      return { kind: "strip", beforeTabId: before?.dataset.tabId ?? null, markerX: edge - bar.left - 1 };
    }

    const area = surfaceArea?.getBoundingClientRect();
    if (!area || !inside(area, x, y)) return { kind: "away" };
    const fx = (x - area.left) / area.width;
    const fy = (y - area.top) / area.height;
    const pane = (splitLayout && paneAt(splitLayout.panes, fx, fy)) ?? {
      tabId: terminalState.activeTabId ?? tabId,
      rect: { x: 0, y: 0, w: 1, h: 1 },
    };
    // A pane can't be split against itself: the middle is the only target there.
    const side =
      pane.tabId === tabId ? "center" : dropZone((fx - pane.rect.x) / pane.rect.w, (fy - pane.rect.y) / pane.rect.h);
    return { kind: "pane", tabId: pane.tabId, side, rect: zoneRect(pane.rect, side) };
  }

  async function applyDrop(tabId: string, target: DropTarget) {
    try {
      if (target.kind === "strip") moveTab(tabId, target.beforeTabId);
      else if (target.kind === "group") addTabToGroup(tabId, target.groupId);
      else if (target.kind === "pane" && target.side !== "center") {
        splitTab(tabId, target.tabId, target.side);
        if (homeActive) appState.goToTerminals();
      } else await popOutTab(tabId);
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  function dragHandlers(tabId: string): TabDragHandlers {
    return {
      move: (x, y) => {
        drag = { tabId, target: dropTargetAt(tabId, x, y) };
      },
      drop: async (x, y) => {
        const target = dropTargetAt(tabId, x, y);
        drag = null;
        await applyDrop(tabId, target);
      },
      cancel: () => {
        drag = null;
      },
    };
  }

  function popsOut(tabId: string): boolean {
    const target = drag?.tabId === tabId ? drag.target : null;
    return target?.kind === "away" || (target?.kind === "pane" && target.side === "center");
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

  <div data-terminals-view class="flex min-h-0 w-full flex-1 flex-col overflow-hidden bg-surface-terminal">
    <!-- The accessible title belongs to the terminal surface; on home the host
         list below carries its own heading. -->
    {#if !homeActive && consoleInView}
      <h1 class="sr-only">Terminals</h1>
    {/if}
    <!-- Tab strip + active-session toolbar -->
    <div
      bind:this={stripBar}
      class="relative z-20 flex h-9 shrink-0 select-none items-stretch gap-1 border-b border-border bg-surface pr-2"
      use:tabBarReattachDrop={bringBackSession}
    >
      <div
        bind:this={tabStrip}
        class="scrollbar-none flex h-full min-w-0 items-stretch gap-0 overflow-x-auto"
        role="tablist"
        aria-label="Terminal sessions"
        tabindex="-1"
        use:horizontalWheel
      >
        <!-- Pinned home tab: the host list, the Console's starting surface. -->
        <button
          type="button"
          role="tab"
          aria-selected={homeActive}
          class="sticky left-0 z-10 flex h-full shrink-0 items-center gap-1.5 border-r border-border-subtle pl-2.5 pr-2.5 text-xs font-medium transition-colors duration-fast
            {homeActive
              ? 'bg-surface-terminal text-primary'
              : 'bg-surface text-muted hover:bg-surface-hover hover:text-primary'}"
          title="Hosts — the host list"
          onclick={onGoHome}
        >
          {#if homeActive}
            <span class="pointer-events-none absolute inset-x-0 top-0 h-0.5 bg-accent" aria-hidden="true"></span>
          {/if}
          <House size={14} class="shrink-0" aria-hidden="true" />
          <span>Hosts</span>
        </button>

        {#each stripItems as item (item.kind === "tab" ? item.tab.id : `group:${item.group.id}`)}
          {#if item.kind === "group"}
            {@const group = item.group}
            {@const label = tabGroupLabel(group)}
            <button
              type="button"
              data-group-id={group.id}
              class="tab-group-chip mx-1 flex h-6 shrink-0 self-center items-center gap-1.5 rounded-md px-2 text-xs font-medium text-primary"
              class:tab-group-chip-target={drag?.target.kind === "group" && drag.target.groupId === group.id}
              style:--group-color={tabGroupColorVar(group.color)}
              aria-expanded={!group.collapsed}
              aria-label="{label}, {item.count} {item.count === 1 ? 'session' : 'sessions'}"
              title="{label} — click to {group.collapsed ? 'expand' : 'collapse'}, right-click to edit"
              onclick={() => toggleTabGroupCollapsed(group.id)}
              oncontextmenu={(e) => openGroupMenu(e, group.id)}
              onkeydown={(e) => {
                if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) openGroupMenu(e, group.id);
              }}
            >
              {#if group.name.trim()}
                <span class="max-w-32 truncate">{group.name.trim()}</span>
              {:else}
                <span class="tab-group-dot" aria-hidden="true"></span>
              {/if}
              {#if group.collapsed}
                <span class="tabular-nums text-secondary">{item.count}</span>
              {/if}
            </button>
          {:else}
            <StripTab
              tab={item.tab}
              group={item.group}
              active={!homeActive && terminalState.activeTabId === item.tab.id}
              inSplit={splitTabIds.has(item.tab.id)}
              poppingOut={popsOut(item.tab.id)}
              drag={dragHandlers(item.tab.id)}
              onMenu={(e) => openTabMenu(e, item.tab.id)}
              onSelect={handleSelectTab}
            />
          {/if}
        {/each}

        {#if awaySessions.length > 0}
          <span class="mx-1 h-4 w-px shrink-0 self-center bg-border" aria-hidden="true"></span>
          {#each awaySessions as session (session.sessionId)}
            <div
              class="group flex h-7 shrink-0 self-center cursor-grab items-center gap-1.5 rounded-md border border-dashed border-border pl-2 pr-0.5 text-xs text-muted transition-colors duration-fast hover:border-border-hover hover:text-primary"
              draggable="true"
              title={session.kind === "popout"
                ? `${session.name} — in its own window. Click to focus, drag here to dock.`
                : `${session.name} — running in the background. Click to reattach.`}
              ondragstart={(event) => startAwayDrag(event, session)}
              ondragend={endAwayDrag}
              onclick={() => activateAwaySession(session)}
              ondblclick={() => void bringBackSession(session)}
              onkeydown={(e) => {
                if (e.key === "Enter") activateAwaySession(session);
              }}
              role="button"
              tabindex="0"
            >
              {#if session.kind === "popout"}
                <AppWindow size={14} class="shrink-0" />
              {:else}
                <Unlink size={14} class="shrink-0" />
              {/if}
              <span class="max-w-32 truncate">{session.name}</span>
              <button
                type="button"
                class="btn-icon btn-icon-sm size-6"
                aria-label={session.kind === "popout" ? `Dock ${session.name}` : `Reattach ${session.name}`}
                title={session.kind === "popout" ? "Dock to this window" : "Reattach"}
                onclick={(e) => {
                  e.stopPropagation();
                  void bringBackSession(session);
                }}
              >
                <Link2 size={14} />
              </button>
            </div>
          {/each}
        {/if}
      </div>
      {#if drag && drag.target.kind === "strip"}
        <div
          class="pointer-events-none absolute bottom-1.5 top-1.5 z-30 w-0.5 rounded-full bg-accent"
          style:left="{drag.target.markerX}px"
          aria-hidden="true"
        ></div>
      {/if}

      <div class="flex shrink-0 items-center self-center">
        <button
          bind:this={newSessionBtn}
          type="button"
          class="btn-icon {showLauncher && !launcherSplit ? 'bg-surface-hover text-primary' : ''}"
          aria-label="New session"
          aria-expanded={showLauncher && !launcherSplit}
          title="New session"
          onclick={(e) => openLauncher(null, e.currentTarget)}
        >
          <Plus size={14} />
        </button>
      </div>

      <div class="min-w-2 flex-1 self-stretch" aria-hidden="true"></div>

      <div class="flex shrink-0 items-center gap-1 self-center">
        {#if activeTab && !homeActive}
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

          <button
            bind:this={splitRightBtn}
            type="button"
            class="btn-icon {showLauncher && launcherSplit === 'right' ? 'bg-surface-hover text-primary' : ''}"
            aria-label="Split right"
            aria-expanded={showLauncher && launcherSplit === "right"}
            title="Open a session to the right"
            onclick={(e) => openLauncher("right", e.currentTarget)}
          >
            <Columns2 size={14} />
          </button>
          <button
            bind:this={splitDownBtn}
            type="button"
            class="btn-icon {showLauncher && launcherSplit === 'bottom' ? 'bg-surface-hover text-primary' : ''}"
            aria-label="Split down"
            aria-expanded={showLauncher && launcherSplit === "bottom"}
            title="Open a session below"
            onclick={(e) => openLauncher("bottom", e.currentTarget)}
          >
            <Rows2 size={14} />
          </button>
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
              launcherAnchor = null;
              showMoreMenu = !showMoreMenu;
            }}
          >
            <MoreHorizontal size={14} />
          </button>
          {#if showMoreMenu}
            <div class="fixed inset-0 z-40" role="presentation" onclick={closeMenus}></div>
            <div class="popover absolute right-0 top-full mt-1 w-56" role="menu">
              {#if activeTab && !homeActive}
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
                <div class="flex items-center justify-between gap-2 px-2.5 py-1.5" role="group" aria-label="Font size">
                  <span class="text-xs text-secondary">Font size</span>
                  <div class="flex items-center gap-0.5 rounded-md border border-border">
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      aria-label="Decrease font size"
                      title="Smaller text (Ctrl −)"
                      onclick={() => updateTerminalFontSize(getTerminalFontSize() - 1)}
                    >
                      <Minus size={14} />
                    </button>
                    <span class="w-6 text-center font-mono text-xs tabular-nums text-primary" title="Font size">
                      {getTerminalFontSize()}
                    </span>
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      aria-label="Increase font size"
                      title="Larger text (Ctrl +)"
                      onclick={() => updateTerminalFontSize(getTerminalFontSize() + 1)}
                    >
                      <Plus size={14} />
                    </button>
                  </div>
                </div>
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

      {#if showLauncher}
        <div class="fixed inset-0 z-40" role="presentation" onclick={closeMenus}></div>
        {#key launcherSplit}
          <HostLauncher
            {connections}
            split={launcherSplit}
            anchor={launcherAnchor}
            onConnect={handleConnect}
            onOpenGroup={handleOpenGroup}
          />
        {/key}
      {/if}
    </div>

    <!-- Terminal surfaces stay mounted (xterm buffers) and keep their size:
         the home surface is layered above them, never replacing them. -->
    <div class="relative min-h-0 flex-1 overflow-hidden">
      <div
        bind:this={surfaceArea}
        class="absolute inset-0 flex flex-col overflow-hidden bg-surface-terminal"
        class:invisible={homeActive}
        aria-hidden={homeActive}
      >
        {#each terminalState.tabs as tab (tab.id)}
          <TerminalPane
            {tab}
            rect={paneRects.get(tab.id)}
            active={terminalState.activeTabId === tab.id}
            {connections}
          />
        {/each}

        {#if splitLayout}
          <SplitDividers dividers={splitLayout.dividers} area={surfaceArea} />
        {/if}

        {#if drag && drag.target.kind === "pane"}
          {@const zone = drag.target}
          <div
            class="split-drop-zone pointer-events-none absolute z-30 flex items-center justify-center rounded-md"
            style:left={cssPercent(zone.rect.x)}
            style:top={cssPercent(zone.rect.y)}
            style:width={cssPercent(zone.rect.w)}
            style:height={cssPercent(zone.rect.h)}
            aria-hidden="true"
          >
            <span class="rounded-md border border-border-floating bg-surface-raised px-2 py-1 text-xs text-primary shadow-md">
              {zone.side === "center" ? "Open in new window" : `Split ${zone.side}`}
            </span>
          </div>
        {/if}
      </div>

      {#if homeInView}
        <div class="absolute inset-0 flex min-h-0 flex-col overflow-hidden bg-surface">
          {@render home()}
        </div>
      {/if}
    </div>
  </div>

{#if tabMenu}
  {@const menuTab = terminalState.tabs.find((t) => t.id === tabMenu?.tabId)}
  {#if menuTab}
    <TabMenu
      tab={menuTab}
      x={tabMenu.x}
      y={tabMenu.y}
      inSplit={splitTabIds.has(menuTab.id)}
      onClose={closeFloatingMenu}
      onSplitNew={(side) => openLauncher(side)}
      onGroupCreated={(groupId, at) => void editNewGroup(groupId, at)}
    />
  {/if}
{/if}

{#if groupMenu}
  {@const group = groupsById.get(groupMenu.groupId)}
  {#if group}
    <GroupMenu {group} x={groupMenu.x} y={groupMenu.y} onClose={closeFloatingMenu} />
  {/if}
{/if}

<style>
  /* Drop target highlight while dragging a pop-out/background session. */
  :global(.tab-bar-drop-active) {
    background: color-mix(in srgb, var(--color-accent) 6%, var(--color-surface)) !important;
    outline: 1px dashed color-mix(in srgb, var(--color-accent) 60%, transparent);
    outline-offset: -3px;
  }
  /* Tab dragged where releasing opens it in its own window. */
  :global(.tab-dragging-out) {
    opacity: 0.55;
    outline: 1px dashed var(--color-accent);
    outline-offset: -1px;
  }
  :global(.tab-dragging) {
    cursor: grabbing;
  }
  .tab-group-chip {
    background: color-mix(in srgb, var(--group-color) 22%, var(--color-surface));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--group-color) 45%, transparent);
    transition: background-color var(--transition-duration-fast);
  }
  .tab-group-chip:hover {
    background: color-mix(in srgb, var(--group-color) 32%, var(--color-surface));
  }
  .tab-group-chip-target {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }
  .split-drop-zone {
    background: var(--color-accent-muted);
    outline: 2px dashed color-mix(in srgb, var(--color-accent) 70%, transparent);
    outline-offset: -2px;
  }
</style>
