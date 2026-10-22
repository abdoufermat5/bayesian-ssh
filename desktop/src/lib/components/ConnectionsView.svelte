<script lang="ts" module>
  import type { SourceFilterValue } from "$lib/components/hosts/SourceBadge.svelte";

  // Kept across remounts (switching views), like the tag filter in appState.
  let sourceFilter = $state<SourceFilterValue>("all");
</script>

<script lang="ts">
  import { tick } from "svelte";
  import { Activity, Download, LayoutGrid, List, Plus, Search, Server, SquareTerminal, Upload, X } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Connection, Prediction } from "$lib/types";
  import { GLOBAL_SHORTCUTS, HOST_SHORTCUTS, IS_MAC, MOD, type Combo } from "$lib/shortcuts";
  import { predictionDetail, predictionReason } from "$lib/utils/prediction";
  import { toSshCommand } from "$lib/utils/sshCommand";
  import { formatDateTime, formatRelative } from "$lib/utils/timezone";
  import { notify } from "$lib/stores/notifications.svelte";
  import { appState } from "$lib/stores/appState.svelte";
  import { requestFilesHost } from "$lib/stores/filesHost.svelte";
  import TagFilter from "./hosts/TagFilter.svelte";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import SourceBadge, {
    loadSourceIndex,
    matchesSource,
    sourceOf,
    type SourceInfo,
  } from "$lib/components/hosts/SourceBadge.svelte";
  import SourceFilter from "$lib/components/hosts/SourceFilter.svelte";
  import TagList from "$lib/components/hosts/TagList.svelte";
  import HostActions from "$lib/components/hosts/HostActions.svelte";
  import ShareToTeamModal from "$lib/components/modals/ShareToTeamModal.svelte";

  interface Props {
    connections: Connection[];
    viewMode: "list" | "grid";
    selectedHostIndex: number;
    copiedId: string | null;
    justDuplicatedId: string | null;
    timezone: string;
    searchQuery?: string;
    selectedTag?: string | null;
    allTags: string[];
    onSelectHost: (index: number) => void;
    /** Without `newTab`, an open session to the same host is brought forward. */
    onConnect: (conn: Connection, options?: { newTab?: boolean }) => void | Promise<void>;
    onEdit: (conn: Connection) => void;
    onDelete: (conn: Connection) => void;
    onDuplicate: (conn: Connection) => void;
    onCopyCommand: (text: string, id: string) => void;
    onAddHost: () => void;
    onImportSshConfig: () => void;
    onOpenBatchExec: () => void;
    onOpenUpload: () => void;
  }

  let {
    connections,
    viewMode = $bindable("list"),
    selectedHostIndex,
    justDuplicatedId,
    timezone,
    searchQuery = $bindable(""),
    selectedTag = $bindable(null),
    allTags,
    onSelectHost,
    onConnect,
    onEdit,
    onDelete,
    onDuplicate,
    onCopyCommand,
    onAddHost,
    onImportSshConfig,
    onOpenBatchExec,
    onOpenUpload,
  }: Props = $props();

  type SortKey = "bayesian" | "name" | "recent" | "host";
  const SORT_OPTIONS = [
    { value: "bayesian", label: "Smart rank", description: "Frequency and recency" },
    { value: "recent", label: "Last used" },
    { value: "name", label: "Name" },
    { value: "host", label: "Address" },
  ];

  /** Below this width the tags cell shows one tag and "+N". */
  const WIDE_MIN_PX = 1180;

  type PingResult = { latency_ms: number; success: boolean };

  let pinging = $state(false);
  let pingResults = $state.raw<Record<string, PingResult>>({});
  let connectingHostId = $state<string | null>(null);
  let sortBy = $state<SortKey>("bayesian");
  let suggestions = $state.raw<Prediction[]>([]);
  let shareTarget = $state.raw<Connection | null>(null);
  let innerWidth = $state(typeof window === "undefined" ? 1280 : window.innerWidth);
  // Re-render relative timestamps once a minute, not per frame.
  let now = $state(Date.now());

  const tagLimit = $derived(innerWidth < WIDE_MIN_PX ? 1 : 2);
  const showFiles = $derived(appState.settings.enable_sftp !== false);

  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    void connections;
    let cancelled = false;
    invoke<Prediction[]>("predict_connections", { limit: 3 })
      .then((list) => {
        if (!cancelled) {
          suggestions = (list ?? []).filter((p) => p.breakdown.total_w > 0 || Boolean(p.connection.last_used));
        }
      })
      .catch(() => {
        if (!cancelled) suggestions = [];
      });
    return () => {
      cancelled = true;
    };
  });

  // ---- Provenance (inventory sources, teams) -------------------------
  // Only fetched when some host actually comes from an inventory source.
  let sourceIndex = $state.raw<Record<string, SourceInfo>>({});
  const hasSyncedHosts = $derived(connections.some((c) => c.source_id));

  $effect(() => {
    if (!hasSyncedHosts) return;
    void connections;
    let cancelled = false;
    loadSourceIndex().then((index) => {
      if (!cancelled) sourceIndex = index;
    });
    return () => {
      cancelled = true;
    };
  });

  /** Sources that have hosts in this list, teams first, then by name. */
  const filterSources = $derived.by(() => {
    const ids = new Set(connections.map((c) => c.source_id).filter((id): id is string => Boolean(id)));
    return [...ids]
      .map((id) => sourceOf(id, sourceIndex) as SourceInfo)
      .sort((a, b) => Number(b.kind === "team") - Number(a.kind === "team") || a.name.localeCompare(b.name));
  });

  const sourceCounts = $derived.by(() => {
    const counts: Record<string, number> = { all: connections.length, local: 0 };
    for (const c of connections) {
      const key = c.source_id || "local";
      counts[key] = (counts[key] ?? 0) + 1;
    }
    return counts;
  });

  // A deleted source can't stay selected, nor can a filter whose chips are gone.
  $effect(() => {
    if (sourceFilter === "all") return;
    const sourceGone =
      sourceFilter !== "local" && Object.keys(sourceIndex).length > 0 && !(sourceFilter in sourceIndex);
    const noSources = !hasSyncedHosts && !searchQuery.trim() && !selectedTag;
    if (sourceGone || noSources) sourceFilter = "all";
  });

  const columnCount = $derived(hasSyncedHosts ? 6 : 5);

  function teamNameOf(conn: Connection): string | null {
    const source = sourceOf(conn.source_id, sourceIndex);
    return source?.kind === "team" ? source.name : null;
  }

  async function handleConnectHost(conn: Connection, newTab = false) {
    connectingHostId = conn.id;
    try {
      await onConnect(conn, { newTab });
    } finally {
      connectingHostId = null;
    }
  }

  function openFiles(conn: Connection) {
    if (!showFiles) {
      notify("Files is turned off in Settings (Terminal & files)", "info");
      return;
    }
    requestFilesHost(conn.id);
    appState.handleTabChange("sftp");
  }

  async function pingAllHosts() {
    pinging = true;
    try {
      const results = await invoke<Array<{ connection_id: string; success: boolean; latency_ms: number }>>(
        "ping_all_connections",
      );
      const map: Record<string, PingResult> = {};
      let reachable = 0;
      for (const r of results) {
        map[r.connection_id] = { latency_ms: r.latency_ms, success: r.success };
        if (r.success) reachable++;
      }
      pingResults = map;
      notify(`${reachable} of ${results.length} hosts reachable`, "success");
    } catch (err) {
      notify(`Ping failed: ${err}`, "error");
    } finally {
      pinging = false;
    }
  }

  // The backend already applies search + tag filters (Bayesian ranking);
  // the source filter is local, and the view only re-sorts when a
  // non-default order is chosen.
  const rows = $derived.by(() => {
    const list = connections
      .map((conn, originalIndex) => ({ conn, originalIndex }))
      .filter(({ conn }) => matchesSource(conn.source_id, sourceFilter));
    if (sortBy === "name") list.sort((a, b) => a.conn.name.localeCompare(b.conn.name));
    else if (sortBy === "recent") list.sort((a, b) => (b.conn.last_used ?? "").localeCompare(a.conn.last_used ?? ""));
    else if (sortBy === "host") list.sort((a, b) => a.conn.host.localeCompare(b.conn.host));
    return list;
  });

  const hasFilters = $derived(Boolean(searchQuery.trim() || selectedTag || sourceFilter !== "all"));
  const pingCount = $derived(Object.keys(pingResults).length);
  const reachableCount = $derived(Object.values(pingResults).filter((r) => r.success).length);

  // ---- Windowed rendering -------------------------------------------
  // Rendering every host row is O(n) DOM, so the list only mounts the rows
  // in view plus an overscan; spacer rows keep the scroll height. The grid
  // mounts cards progressively as you scroll.
  const OVERSCAN = 12;
  const GRID_PAGE = 96;
  let bodyEl = $state<HTMLDivElement>();
  let tableEl = $state<HTMLTableElement>();
  let scrollTop = $state(0);
  let viewportH = $state(800);
  let rowH = $state(40);
  let gridLimit = $state(GRID_PAGE);

  $effect(() => {
    const el = bodyEl;
    if (!el) return;
    const ro = new ResizeObserver(() => (viewportH = el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  });

  // Measure the real row height whenever the mounted rows change
  // (density, font size and zoom dependent); spacer math relies on it.
  $effect(() => {
    void visibleRows.length;
    const first = tableEl?.querySelector<HTMLTableRowElement>("tbody tr[data-host-row]");
    if (first && first.offsetHeight > 0 && first.offsetHeight !== rowH) rowH = first.offsetHeight;
  });

  // A new result set starts the grid from the first page again.
  $effect(() => {
    void rows;
    gridLimit = GRID_PAGE;
  });

  /** Table border + sticky header height above the first row. */
  const tableTop = 37;
  // Clamp to the current list: after a filter shrinks it, scrollTop is stale
  // until the browser clamps it and fires `scroll`, and the window must not
  // point past the end in between.
  const windowSize = $derived(Math.ceil(viewportH / rowH) + 2 * OVERSCAN);
  const windowStart = $derived(
    Math.min(
      Math.max(0, Math.floor((scrollTop - tableTop) / rowH) - OVERSCAN),
      Math.max(0, rows.length - windowSize),
    ),
  );
  const windowEnd = $derived(Math.min(rows.length, windowStart + windowSize));
  const visibleRows = $derived(rows.slice(windowStart, windowEnd));

  function onBodyScroll() {
    if (!bodyEl) return;
    scrollTop = bodyEl.scrollTop;
    if (
      viewMode === "grid" &&
      gridLimit < rows.length &&
      bodyEl.scrollTop + bodyEl.clientHeight > bodyEl.scrollHeight - 600
    ) {
      gridLimit += GRID_PAGE;
    }
  }

  function statusOf(conn: Connection): { cls: string; label: string } {
    const ping = pingResults[conn.id];
    if (ping) {
      return ping.success
        ? { cls: "status-dot-success", label: `Reachable · ${ping.latency_ms} ms` }
        : { cls: "status-dot-error", label: "Unreachable" };
    }
    return { cls: "status-dot-offline", label: "Not checked — use Ping" };
  }

  function lastUsedLabel(conn: Connection) {
    void now;
    return conn.last_used ? formatRelative(conn.last_used) : "Never";
  }

  function clearFilters() {
    searchQuery = "";
    selectedTag = null;
    sourceFilter = "all";
  }

  // ---- Keyboard: one tab stop, roving tabindex, HOST_SHORTCUTS --------
  // The list is an ARIA grid: the selected row is the only tab stop; ←/→
  // walk the row's actions, ↑/↓/J/K move between rows, and the verbs in
  // HOST_SHORTCUTS act on the selected row.

  const selectedId = $derived(connections[selectedHostIndex]?.id ?? null);

  /** The row that takes Tab: the selection when it is mounted, else the first mounted one. */
  const tabStopId = $derived.by(() => {
    const mounted = viewMode === "list" ? visibleRows : rows.slice(0, gridLimit);
    if (selectedId && mounted.some((r) => r.conn.id === selectedId)) return selectedId;
    return mounted[0]?.conn.id ?? null;
  });

  const KEY_NAMES: Record<string, string> = { "↑": "ArrowUp", "↓": "ArrowDown" };

  function matchesCombo(e: KeyboardEvent, combo: Combo): boolean {
    const key = KEY_NAMES[combo[combo.length - 1]] ?? combo[combo.length - 1];
    const mods = combo.slice(0, -1);
    const mod = IS_MAC ? e.metaKey : e.ctrlKey;
    const otherMod = IS_MAC ? e.ctrlKey : e.metaKey;
    if (mod !== mods.includes(MOD) || otherMod || e.altKey || e.shiftKey !== mods.includes("Shift")) return false;
    return key.length === 1 ? e.key.toLowerCase() === key.toLowerCase() : e.key === key;
  }

  /** Host keys work outside inputs, buttons, menus, modals and the terminal. */
  function hostKeysActive(): boolean {
    if (document.querySelector('.modal-overlay, [aria-modal="true"]')) return false;
    const active = document.activeElement as HTMLElement | null;
    if (!active || active === document.body) return true;
    if (active.closest('.xterm, [role="menu"], [role="dialog"], [role="listbox"]')) return false;
    return !active.matches(
      "input, textarea, select, button, a[href], [contenteditable='true'], [role='combobox'], [role='option'], [role='tab'], [role='menuitem']",
    );
  }

  function selectedDisplayIndex() {
    return rows.findIndex(({ originalIndex }) => originalIndex === selectedHostIndex);
  }

  /** Keep the keyboard selection visible; works for rows not yet mounted. */
  function scrollRowIntoView(displayIndex: number) {
    const el = bodyEl;
    if (!el) return;
    if (viewMode === "grid") {
      if (displayIndex >= gridLimit) gridLimit = displayIndex + GRID_PAGE;
      return;
    }
    const top = tableTop + displayIndex * rowH;
    if (top < el.scrollTop + 36) el.scrollTop = top - 36;
    else if (top + rowH > el.scrollTop + el.clientHeight) el.scrollTop = top + rowH - el.clientHeight;
    // Mount the target row now rather than on the next scroll event.
    scrollTop = el.scrollTop;
  }

  async function focusRow(id: string) {
    await tick();
    const row = bodyEl?.querySelector<HTMLElement>(`[data-host-focus][data-host-id="${CSS.escape(id)}"]`);
    row?.focus({ preventScroll: viewMode === "list" });
    if (viewMode === "grid") row?.scrollIntoView({ block: "nearest" });
  }

  function moveSelection(delta: 1 | -1) {
    if (rows.length === 0) return;
    const current = selectedDisplayIndex();
    const next =
      current === -1
        ? delta > 0
          ? 0
          : rows.length - 1
        : Math.min(Math.max(current + delta, 0), rows.length - 1);
    onSelectHost(rows[next].originalIndex);
    scrollRowIntoView(next);
    void focusRow(rows[next].conn.id);
  }

  function runHostVerb(id: string, conn: Connection) {
    switch (id) {
      case "host-connect":
        void handleConnectHost(conn);
        break;
      case "host-connect-tab":
        void handleConnectHost(conn, true);
        break;
      case "host-files":
        openFiles(conn);
        break;
      case "host-copy":
        onCopyCommand(toSshCommand(conn), conn.id);
        break;
      case "host-edit":
        onEdit(conn);
        break;
      case "host-share":
        shareTarget = conn;
        break;
      case "host-delete":
        onDelete(conn);
        break;
    }
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || rows.length === 0 || !hostKeysActive()) return;
    const shortcut = HOST_SHORTCUTS.find((s) => s.combos.some((c) => matchesCombo(e, c)));
    if (!shortcut) return;

    if (shortcut.id === "host-move") {
      e.preventDefault();
      moveSelection(e.key === "ArrowDown" || e.key.toLowerCase() === "j" ? 1 : -1);
      return;
    }

    const index = selectedDisplayIndex();
    if (index === -1) return;
    if (shortcut.id === "host-connect" || shortcut.id === "host-connect-tab") {
      if (connectingHostId !== null) return;
    }
    e.preventDefault();
    runHostVerb(shortcut.id, rows[index].conn);
  }

  /** Focusable controls of a row, outside its popovers. */
  function rowControls(row: HTMLElement): HTMLElement[] {
    return Array.from(row.querySelectorAll<HTMLElement>("button:not([disabled])")).filter(
      (el) => !el.closest('[role="menu"], [role="dialog"]'),
    );
  }

  /** ←/→ between a row and its actions, ↑/↓ out of an action, Esc back to the row. */
  function onGridKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
    const target = e.target as HTMLElement;
    if (target.closest('[role="menu"], [role="dialog"]')) return;
    const row = target.closest<HTMLElement>("[data-host-focus]");
    if (!row) return;
    const controls = rowControls(row);

    if (target === row) {
      if (e.key === "ArrowRight" && controls.length) {
        e.preventDefault();
        controls[0].focus();
      }
      return;
    }

    const i = controls.indexOf(target);
    if (i === -1) return;
    if (e.key === "ArrowRight") {
      e.preventDefault();
      controls[Math.min(i + 1, controls.length - 1)].focus();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      (i === 0 ? row : controls[i - 1]).focus();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      moveSelection(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      row.focus();
    }
  }

  function onRowFocusIn(originalIndex: number) {
    if (selectedHostIndex !== originalIndex) onSelectHost(originalIndex);
  }

  function selectRow(originalIndex: number, e: MouseEvent) {
    onSelectHost(originalIndex);
    // Clicks on the row itself make it the focused tab stop; clicks on its
    // buttons keep focus where the browser put it.
    const row = e.currentTarget as HTMLElement;
    if (!(e.target as HTMLElement).closest("button")) row.focus({ preventScroll: true });
  }

  // Screen-reader help for the grid, generated from the shortcut table.
  const SPOKEN_KEYS: Record<string, string> = { "↑": "Up arrow", "↓": "Down arrow" };
  const gridHelp = [
    `${(HOST_SHORTCUTS.find((s) => s.id === "host-move")?.combos ?? []).map((c) => c.map((k) => SPOKEN_KEYS[k] ?? k).join("+")).join(", ")}: move between hosts`,
    "Right and left arrows: reach a host's actions",
    ...HOST_SHORTCUTS.filter((s) => s.id !== "host-move").map((s) => `${s.combos[0].join("+")}: ${s.label.replace(/…$/, "")}`),
  ].join(". ");

  const newHostKeys = GLOBAL_SHORTCUTS.find((s) => s.id === "new-host")?.combos[0].join("+") ?? "";
  const filterKeys = GLOBAL_SHORTCUTS.find((s) => s.id === "filter")?.combos[0].join("+") ?? "";
</script>

<svelte:window onkeydown={onWindowKeydown} bind:innerWidth />

{#snippet kindBadges(conn: Connection)}
  {#if conn.use_kerberos}
    <span class="badge badge-neutral" title="Kerberos (GSSAPI) authentication">Kerberos</span>
  {/if}
  {#if conn.bastion}
    <span class="badge badge-neutral" title={`Via jump host ${conn.bastion_user ? `${conn.bastion_user}@` : ""}${conn.bastion}`}>
      Jump
    </span>
  {/if}
  {#if conn.stale_since}
    <span
      class="badge badge-warning"
      title={`Missing from syncs since ${formatDateTime(conn.stale_since, timezone)}. Deleted only by a sync with --prune.`}
    >
      Stale · {formatRelative(conn.stale_since, now)}
    </span>
  {/if}
{/snippet}

{#snippet actions(conn: Connection)}
  <HostActions
    {conn}
    connecting={connectingHostId === conn.id}
    busy={connectingHostId !== null}
    {showFiles}
    teamName={teamNameOf(conn)}
    onConnect={() => void handleConnectHost(conn)}
    onFiles={() => openFiles(conn)}
    onCopy={() => onCopyCommand(toSshCommand(conn), conn.id)}
    onEdit={() => onEdit(conn)}
    onDuplicate={() => onDuplicate(conn)}
    onShare={() => (shareTarget = conn)}
    onDelete={() => onDelete(conn)}
  />
{/snippet}

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Hosts</h1>
      <span class="text-sm tabular-nums text-muted">
        {#if hasFilters}{rows.length} matching{:else}{rows.length}{/if}
      </span>
    </div>
    <div class="view-actions">
      <button
        type="button"
        class="btn btn-ghost"
        onclick={pingAllHosts}
        disabled={pinging || connections.length === 0}
        title="Check which hosts are reachable"
      >
        {#if pinging}
          <span class="spinner size-3"></span>
        {:else}
          <Activity size={14} />
        {/if}
        {pingCount > 0 ? `${reachableCount}/${pingCount} up` : "Ping"}
      </button>
      <button
        type="button"
        class="btn btn-ghost"
        onclick={onOpenBatchExec}
        disabled={connections.length === 0}
        title="Run a command on several hosts"
      >
        <SquareTerminal size={14} />
        Batch run
      </button>
      <button
        type="button"
        class="btn btn-ghost"
        onclick={onOpenUpload}
        disabled={connections.length === 0}
        title="Upload files to several hosts"
      >
        <Upload size={14} />
        Upload files
      </button>
      <button type="button" class="btn btn-primary" onclick={onAddHost} title="New host ({newHostKeys})">
        <Plus size={15} />
        New host
      </button>
    </div>
  </header>

  <div class="view-toolbar">
    <label class="search-box w-64 lg:w-80">
      <Search size={14} class="shrink-0" />
      <input
        type="text"
        class="search-input"
        placeholder="Filter hosts"
        bind:value={searchQuery}
        spellcheck="false"
        autocomplete="off"
      />
      {#if searchQuery}
        <button
          type="button"
          class="btn-icon btn-icon-sm -mr-1"
          onclick={() => (searchQuery = "")}
          aria-label="Clear filter"
        >
          <X size={13} />
        </button>
      {:else}
        <kbd class="kbd">{filterKeys}</kbd>
      {/if}
    </label>

    {#if allTags.length > 0}
      <TagFilter tags={allTags} selected={selectedTag} onSelect={(tag) => (selectedTag = tag)} />
    {:else}
      <div class="flex-1"></div>
    {/if}

    <div class="flex shrink-0 items-center gap-2">
      <div class="w-[168px]">
        <CustomSelect
          label="Sort"
          options={SORT_OPTIONS}
          value={sortBy}
          onChange={(v) => (sortBy = v as SortKey)}
        />
      </div>
      <div class="segmented" role="group" aria-label="Layout">
        <button
          type="button"
          class="segmented-item {viewMode === 'list' ? 'segmented-item-active' : ''}"
          onclick={() => (viewMode = "list")}
          aria-pressed={viewMode === "list"}
          aria-label="List layout"
          title="List"
        >
          <List size={14} />
        </button>
        <button
          type="button"
          class="segmented-item {viewMode === 'grid' ? 'segmented-item-active' : ''}"
          onclick={() => (viewMode = "grid")}
          aria-pressed={viewMode === "grid"}
          aria-label="Grid layout"
          title="Grid"
        >
          <LayoutGrid size={14} />
        </button>
      </div>
    </div>
  </div>

  {#if hasSyncedHosts || sourceFilter !== "all"}
    <div class="flex shrink-0 px-6 pb-3">
      <SourceFilter
        sources={filterSources}
        value={sourceFilter}
        counts={sourceCounts}
        onChange={(v) => (sourceFilter = v)}
      />
    </div>
  {/if}

  {#if !hasFilters && suggestions.length > 0}
    <div class="flex shrink-0 flex-wrap items-center gap-2 px-6 pb-3" role="region" aria-label="Suggested connections">
      <span class="section-label mr-1">Suggested</span>
      {#each suggestions as item (item.connection.id)}
        <button
          type="button"
          class="btn btn-secondary btn-sm"
          onclick={() => handleConnectHost(item.connection)}
          disabled={connectingHostId !== null}
          title={`Connect · ${predictionDetail(item)}`}
        >
          <SquareTerminal size={12} class="text-muted" />
          <span class="font-medium text-primary">{item.connection.name}</span>
          <span class="text-muted">· {predictionReason(item, now)}</span>
        </button>
      {/each}
    </div>
  {/if}

  <p id="hosts-grid-help" class="sr-only">
    {gridHelp}.
  </p>

  <div class="view-body" bind:this={bodyEl} onscroll={onBodyScroll}>
    {#if rows.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Server size={18} /></div>
        {#if hasFilters}
          <div class="empty-state-title">No hosts match</div>
          <p class="empty-state-desc">Nothing matches the current filter. Try another term or clear it.</p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={clearFilters}>Clear filter</button>
          </div>
        {:else}
          <div class="empty-state-title">No hosts yet</div>
          <p class="empty-state-desc">
            Add a server to connect to, or import the hosts already defined in your OpenSSH config.
          </p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={onImportSshConfig}>
              <Download size={14} />
              Import ~/.ssh/config
            </button>
            <button type="button" class="btn btn-primary" onclick={onAddHost}>
              <Plus size={15} />
              New host
            </button>
          </div>
        {/if}
      </div>
    {:else if viewMode === "list"}
      <div class="table-wrap">
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <table
          class="data-table host-table table-fixed"
          bind:this={tableEl}
          role="grid"
          aria-label="Hosts"
          aria-rowcount={rows.length + 1}
          aria-describedby="hosts-grid-help"
          onkeydown={onGridKeydown}
        >
          <thead>
            <tr aria-rowindex={1}>
              <th class="col-name">Name</th>
              <th class="col-address">Address</th>
              <th class="col-tags">Tags</th>
              {#if hasSyncedHosts}<th class="col-source">Source</th>{/if}
              <th class="col-used">Last used</th>
              <th class="col-actions"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#if windowStart > 0}
              <tr aria-hidden="true" style="height: {windowStart * rowH}px"><td colspan={columnCount} class="spacer"></td></tr>
            {/if}
            {#each visibleRows as { conn, originalIndex }, i (conn.id)}
              {@const selected = selectedHostIndex === originalIndex}
              {@const status = statusOf(conn)}
              {@const ping = pingResults[conn.id]}
              <tr
                class="cursor-default {selected ? 'is-selected' : ''} {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
                data-host-row
                data-host-focus
                data-host-id={conn.id}
                data-host-selected={selected}
                tabindex={tabStopId === conn.id ? 0 : -1}
                aria-rowindex={windowStart + i + 2}
                aria-selected={selected}
                aria-label="{conn.name}, {conn.user}@{conn.host}{conn.port !== 22 ? `:${conn.port}` : ''}"
                onclick={(e) => selectRow(originalIndex, e)}
                onfocusin={() => onRowFocusIn(originalIndex)}
                ondblclick={() => handleConnectHost(conn)}
              >
                <td>
                  <div class="flex min-w-0 items-center gap-2.5">
                    <span class="status-dot {status.cls}" title={status.label} role="img" aria-label={status.label}></span>
                    <span class="truncate font-medium {conn.stale_since ? 'text-muted' : 'text-primary'}" title={conn.name}>{conn.name}</span>
                    {@render kindBadges(conn)}
                  </div>
                </td>
                <td>
                  <div class="flex min-w-0 items-center gap-2">
                    <span class="truncate font-mono text-xs" title={`${conn.user}@${conn.host}:${conn.port}`}>
                      <span class="text-muted">{conn.user}@</span>{conn.host}{#if conn.port !== 22}<span class="text-muted">:{conn.port}</span>{/if}
                    </span>
                    {#if ping?.success}
                      <span class="shrink-0 font-mono text-2xs text-success">{ping.latency_ms}ms</span>
                    {/if}
                  </div>
                </td>
                <td>
                  <TagList tags={conn.tags} limit={tagLimit} tabindex={-1} />
                </td>
                {#if hasSyncedHosts}
                  <td>
                    <SourceBadge
                      source={sourceOf(conn.source_id, sourceIndex)}
                      detail={conn.external_id ? `id ${conn.external_id}` : undefined}
                    />
                  </td>
                {/if}
                <td>
                  <span
                    class="block truncate text-xs {conn.last_used ? 'text-secondary' : 'text-muted'}"
                    title={conn.last_used ? formatDateTime(conn.last_used, timezone) : undefined}
                  >
                    {lastUsedLabel(conn)}
                  </span>
                </td>
                <td class="actions-cell">
                  <div class="row-actions">{@render actions(conn)}</div>
                </td>
              </tr>
            {/each}
            {#if windowEnd < rows.length}
              <tr aria-hidden="true" style="height: {(rows.length - windowEnd) * rowH}px"><td colspan={columnCount} class="spacer"></td></tr>
            {/if}
          </tbody>
        </table>
      </div>
    {:else}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div
        class="grid grid-cols-[repeat(auto-fill,minmax(272px,1fr))] gap-3"
        role="grid"
        aria-label="Hosts"
        aria-rowcount={rows.length}
        aria-describedby="hosts-grid-help"
        onkeydown={onGridKeydown}
      >
        {#each rows.slice(0, gridLimit) as { conn, originalIndex }, i (conn.id)}
          {@const selected = selectedHostIndex === originalIndex}
          {@const status = statusOf(conn)}
          <!-- Keys are handled by the grid (arrows) and the window (HOST_SHORTCUTS). -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="host-card card card-interactive group {selected ? 'card-selected is-selected' : ''} {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
            data-host-focus
            data-host-id={conn.id}
            data-host-selected={selected}
            role="row"
            tabindex={tabStopId === conn.id ? 0 : -1}
            aria-rowindex={i + 1}
            aria-selected={selected}
            aria-label="{conn.name}, {conn.user}@{conn.host}{conn.port !== 22 ? `:${conn.port}` : ''}"
            onclick={(e) => selectRow(originalIndex, e)}
            onfocusin={() => onRowFocusIn(originalIndex)}
            ondblclick={() => handleConnectHost(conn)}
          >
            <div class="flex h-full flex-col gap-3 p-4" role="gridcell">
              <div class="flex items-start justify-between gap-2">
                <div class="flex min-w-0 flex-col gap-1">
                  <div class="flex min-w-0 items-center gap-2">
                    <span class="status-dot {status.cls}" title={status.label} role="img" aria-label={status.label}></span>
                    <span class="truncate font-medium {conn.stale_since ? 'text-muted' : 'text-primary'}" title={conn.name}>{conn.name}</span>
                  </div>
                  <span class="truncate font-mono text-xs text-muted" title={`${conn.user}@${conn.host}:${conn.port}`}>
                    {conn.user}@{conn.host}{conn.port !== 22 ? `:${conn.port}` : ""}
                  </span>
                </div>
                <div class="flex shrink-0 gap-1">
                  {@render kindBadges(conn)}
                  {#if conn.source_id}
                    <SourceBadge
                      source={sourceOf(conn.source_id, sourceIndex)}
                      detail={conn.external_id ? `id ${conn.external_id}` : undefined}
                    />
                  {/if}
                </div>
              </div>

              {#if conn.tags.length > 0}
                <TagList tags={conn.tags} limit={4} tabindex={-1} wrap />
              {/if}

              <div class="mt-auto flex items-center justify-between gap-2">
                <span class="text-xs text-muted" title={conn.last_used ? formatDateTime(conn.last_used, timezone) : undefined}>
                  {lastUsedLabel(conn)}
                </span>
                <div class="row-actions">{@render actions(conn)}</div>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if shareTarget}
  <ShareToTeamModal
    connection={shareTarget}
    onClose={() => (shareTarget = null)}
    onShared={() => void appState.reloadConnectionsAfterMutation()}
    onOpenTeams={() => appState.handleTabChange("teams")}
  />
{/if}

<style>
  /* Content-sized columns; the name takes what is left. Rows follow the
     density setting (--row-height, 32/40/48 px). */
  .host-table :global(tbody tr[data-host-row]) {
    height: var(--row-height, 40px);
  }
  .host-table :global(tbody tr[data-host-row] > td) {
    height: var(--row-height, 40px);
    padding-top: 0;
    padding-bottom: 0;
  }
  .host-table :global(td.spacer) {
    height: auto;
    padding: 0;
  }
  .col-address {
    width: 180px;
  }
  .col-tags {
    width: 96px;
  }
  .col-source {
    width: 112px;
  }
  .col-used {
    width: 88px;
  }
  .col-actions {
    width: 104px;
  }
  .host-table :global(td.actions-cell) {
    padding-left: 4px;
    padding-right: 12px;
  }
  @media (min-width: 1180px) {
    .col-address {
      width: 240px;
    }
    .col-tags {
      width: 176px;
    }
    .col-source {
      width: 136px;
    }
    .col-used {
      width: 104px;
    }
  }

  /* Actions stay visible on the selected card and while a row menu is open. */
  .host-card.is-selected .row-actions,
  .row-actions:has(:global([aria-expanded="true"])) {
    opacity: 1;
  }

  /* Keyboard focus on a row: a ring drawn on its cells (outlines on table
     rows don't render in every engine). */
  .host-table :global(tr[data-host-focus]:focus) {
    outline: none;
  }
  .host-table :global(tr[data-host-focus]:focus-visible > td) {
    box-shadow:
      inset 0 2px 0 var(--color-border-focus),
      inset 0 -2px 0 var(--color-border-focus);
  }
  .host-table :global(tr[data-host-focus]:focus-visible > td:first-child) {
    box-shadow:
      inset 2px 0 0 var(--color-accent),
      inset 0 2px 0 var(--color-border-focus),
      inset 0 -2px 0 var(--color-border-focus);
  }
  .host-table :global(tr[data-host-focus]:focus-visible > td:last-child) {
    box-shadow:
      inset -2px 0 0 var(--color-border-focus),
      inset 0 2px 0 var(--color-border-focus),
      inset 0 -2px 0 var(--color-border-focus);
  }
</style>
