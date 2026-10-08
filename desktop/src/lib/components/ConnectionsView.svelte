<script lang="ts">
  import {
    Activity,
    Check,
    Copy,
    CopyPlus,
    Download,
    LayoutGrid,
    List,
    Pencil,
    Plus,
    Search,
    Server,
    SquareTerminal,
    Trash2,
    X,
  } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Connection } from "$lib/types";
  import { toSshCommand } from "$lib/utils/sshCommand";
  import { formatDateTime, formatRelative } from "$lib/utils/timezone";
  import { notify } from "$lib/stores/notifications.svelte";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";

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
    onConnect: (conn: Connection) => void | Promise<void>;
    onEdit: (conn: Connection) => void;
    onDelete: (conn: Connection) => void;
    onDuplicate: (conn: Connection) => void;
    onCopyCommand: (text: string, id: string) => void;
    onAddHost: () => void;
    onImportSshConfig: () => void;
    onOpenBatchExec: () => void;
  }

  let {
    connections,
    viewMode = $bindable("list"),
    selectedHostIndex,
    copiedId,
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
  }: Props = $props();

  type SortKey = "bayesian" | "name" | "recent" | "host";
  const SORT_OPTIONS = [
    { value: "bayesian", label: "Smart rank", description: "Frequency and recency" },
    { value: "recent", label: "Last used" },
    { value: "name", label: "Name" },
    { value: "host", label: "Address" },
  ];

  type PingResult = { latency_ms: number; success: boolean };

  let pinging = $state(false);
  let pingResults = $state.raw<Record<string, PingResult>>({});
  let connectingHostId = $state<string | null>(null);
  let sortBy = $state<SortKey>("bayesian");
  // Re-render relative timestamps once a minute, not per frame.
  let now = $state(Date.now());

  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(timer);
  });

  async function handleConnectHost(conn: Connection) {
    connectingHostId = conn.id;
    try {
      await onConnect(conn);
    } finally {
      connectingHostId = null;
    }
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
  // the view only re-sorts when a non-default order is chosen.
  const rows = $derived.by(() => {
    const list = connections.map((conn, originalIndex) => ({ conn, originalIndex }));
    if (sortBy === "name") list.sort((a, b) => a.conn.name.localeCompare(b.conn.name));
    else if (sortBy === "recent") list.sort((a, b) => (b.conn.last_used ?? "").localeCompare(a.conn.last_used ?? ""));
    else if (sortBy === "host") list.sort((a, b) => a.conn.host.localeCompare(b.conn.host));
    return list;
  });

  const hasFilters = $derived(Boolean(searchQuery.trim() || selectedTag));
  const pingCount = $derived(Object.keys(pingResults).length);
  const reachableCount = $derived(Object.values(pingResults).filter((r) => r.success).length);

  // ---- Windowed rendering -------------------------------------------
  // Rendering every host row is O(n) DOM (≈2 ms/row with its actions), so
  // the list only mounts the rows in view plus an overscan; spacer rows keep
  // the scroll height. The grid mounts cards progressively as you scroll.
  const OVERSCAN = 12;
  const GRID_PAGE = 96;
  let bodyEl = $state<HTMLDivElement>();
  let tableEl = $state<HTMLTableElement>();
  let scrollTop = $state(0);
  let viewportH = $state(800);
  let rowH = $state(44);
  let gridLimit = $state(GRID_PAGE);

  $effect(() => {
    const el = bodyEl;
    if (!el) return;
    const ro = new ResizeObserver(() => (viewportH = el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  });

  // Measure the real row height whenever the mounted rows change
  // (font size / zoom dependent); spacer math relies on it.
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
  }

  function shouldIgnoreHostShortcuts() {
    const active = document.activeElement;
    return (
      document.querySelector(".modal-overlay") !== null ||
      active?.tagName === "INPUT" ||
      active?.tagName === "TEXTAREA" ||
      active?.tagName === "SELECT" ||
      active?.tagName === "BUTTON" ||
      active?.tagName === "A" ||
      active?.getAttribute("contenteditable") === "true"
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
      requestAnimationFrame(() =>
        el.querySelector("[data-host-selected='true']")?.scrollIntoView({ block: "nearest" }),
      );
      return;
    }
    const top = tableTop + displayIndex * rowH;
    if (top < el.scrollTop + 36) el.scrollTop = top - 36;
    else if (top + rowH > el.scrollTop + el.clientHeight) el.scrollTop = top + rowH - el.clientHeight;
  }

  function handleVisibleHostKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || rows.length === 0 || shouldIgnoreHostShortcuts()) return;

    const currentIndex = selectedDisplayIndex();

    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "j" || e.key === "k") {
      e.preventDefault();
      const down = e.key === "ArrowDown" || e.key === "j";
      const nextIndex =
        currentIndex === -1
          ? down
            ? 0
            : rows.length - 1
          : down
            ? Math.min(currentIndex + 1, rows.length - 1)
            : Math.max(currentIndex - 1, 0);
      onSelectHost(rows[nextIndex].originalIndex);
      scrollRowIntoView(nextIndex);
      return;
    }

    if (currentIndex === -1) return;
    const selected = rows[currentIndex].conn;

    if (e.key === "Enter") {
      e.preventDefault();
      void handleConnectHost(selected);
      return;
    }

    if (e.key.toLowerCase() === "e" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      onEdit(selected);
    }
  }
</script>

<svelte:window onkeydown={handleVisibleHostKeydown} />

{#snippet hostActions(conn: Connection)}
  <button
    type="button"
    class="btn-icon"
    onclick={(e) => {
      e.stopPropagation();
      onCopyCommand(toSshCommand(conn), conn.id);
    }}
    title="Copy SSH command"
    aria-label="Copy SSH command for {conn.name}"
  >
    {#if copiedId === conn.id}
      <Check size={14} class="text-success" />
    {:else}
      <Copy size={14} />
    {/if}
  </button>
  <button
    type="button"
    class="btn-icon"
    onclick={(e) => {
      e.stopPropagation();
      onEdit(conn);
    }}
    title="Edit (Ctrl+E)"
    aria-label="Edit {conn.name}"
  >
    <Pencil size={14} />
  </button>
  <button
    type="button"
    class="btn-icon"
    onclick={(e) => {
      e.stopPropagation();
      onDuplicate(conn);
    }}
    title="Duplicate"
    aria-label="Duplicate {conn.name}"
  >
    <CopyPlus size={14} />
  </button>
  <button
    type="button"
    class="btn-icon btn-icon-danger"
    onclick={(e) => {
      e.stopPropagation();
      onDelete(conn);
    }}
    title="Delete"
    aria-label="Delete {conn.name}"
  >
    <Trash2 size={14} />
  </button>
{/snippet}

{#snippet connectButton(conn: Connection)}
  <button
    type="button"
    class="btn btn-secondary btn-sm connect-btn"
    onclick={(e) => {
      e.stopPropagation();
      void handleConnectHost(conn);
    }}
    disabled={connectingHostId !== null}
    title="Connect (Enter)"
  >
    {#if connectingHostId === conn.id}
      <span class="spinner size-3"></span>
      Connecting
    {:else}
      <SquareTerminal size={13} />
      Connect
    {/if}
  </button>
{/snippet}

{#snippet hostBadges(conn: Connection)}
  {#if conn.use_kerberos}
    <span class="badge badge-neutral" title="Kerberos (GSSAPI) authentication">Kerberos</span>
  {/if}
  {#if conn.bastion}
    <span class="badge badge-neutral" title={`Via jump host ${conn.bastion_user ? `${conn.bastion_user}@` : ""}${conn.bastion}`}>
      Jump
    </span>
  {/if}
{/snippet}

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Hosts</h1>
      <span class="text-sm tabular-nums text-muted">
        {#if hasFilters}{connections.length} matching{:else}{connections.length}{/if}
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
      <button type="button" class="btn btn-primary" onclick={onAddHost} title="New host (N)">
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
        <kbd class="kbd">/</kbd>
      {/if}
    </label>

    {#if allTags.length > 0}
      <div class="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto scrollbar-none" role="group" aria-label="Filter by tag">
        <button
          type="button"
          class="chip {selectedTag === null ? 'chip-active' : ''}"
          onclick={() => (selectedTag = null)}
          aria-pressed={selectedTag === null}
        >
          All
        </button>
        {#each allTags as tag (tag)}
          <button
            type="button"
            class="chip {selectedTag === tag ? 'chip-active' : ''}"
            onclick={() => (selectedTag = selectedTag === tag ? null : tag)}
            aria-pressed={selectedTag === tag}
          >
            {tag}
          </button>
        {/each}
      </div>
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
        <table class="data-table table-fixed" bind:this={tableEl} aria-rowcount={rows.length}>
          <thead>
            <tr>
              <th class="w-[28%]">Name</th>
              <th class="w-[28%]">Address</th>
              <th class="hidden xl:table-cell">Tags</th>
              <th class="w-[116px]">Last used</th>
              <th class="w-[236px]"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#if windowStart > 0}
              <tr aria-hidden="true" style="height: {windowStart * rowH}px"><td colspan="5" class="p-0"></td></tr>
            {/if}
            {#each visibleRows as { conn, originalIndex } (conn.id)}
              {@const selected = selectedHostIndex === originalIndex}
              {@const status = statusOf(conn)}
              {@const ping = pingResults[conn.id]}
              <tr
                class="cursor-default {selected ? 'is-selected' : ''} {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
                data-host-row
                data-host-selected={selected}
                onclick={() => onSelectHost(originalIndex)}
                ondblclick={() => handleConnectHost(conn)}
                aria-selected={selected}
              >
                <td>
                  <div class="flex min-w-0 items-center gap-2.5">
                    <span class="status-dot {status.cls}" title={status.label}></span>
                    <span class="truncate font-medium text-primary" title={conn.name}>{conn.name}</span>
                    {@render hostBadges(conn)}
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
                <td class="hidden xl:table-cell">
                  <div class="flex min-w-0 gap-1 overflow-hidden">
                    {#each conn.tags.slice(0, 3) as tag (tag)}
                      <span class="tag">{tag}</span>
                    {/each}
                    {#if conn.tags.length > 3}
                      <span class="tag" title={conn.tags.slice(3).join(", ")}>+{conn.tags.length - 3}</span>
                    {/if}
                  </div>
                </td>
                <td>
                  <span
                    class="text-xs {conn.last_used ? 'text-secondary' : 'text-muted'}"
                    title={conn.last_used ? formatDateTime(conn.last_used, timezone) : undefined}
                  >
                    {lastUsedLabel(conn)}
                  </span>
                </td>
                <td>
                  <div class="flex items-center justify-end gap-1.5">
                    <div class="row-actions">{@render hostActions(conn)}</div>
                    {@render connectButton(conn)}
                  </div>
                </td>
              </tr>
            {/each}
            {#if windowEnd < rows.length}
              <tr aria-hidden="true" style="height: {(rows.length - windowEnd) * rowH}px"><td colspan="5" class="p-0"></td></tr>
            {/if}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(272px,1fr))] gap-3">
        {#each rows.slice(0, gridLimit) as { conn, originalIndex } (conn.id)}
          {@const selected = selectedHostIndex === originalIndex}
          {@const status = statusOf(conn)}
          <div
            class="card card-interactive group flex flex-col gap-3 p-4 {selected ? 'card-selected' : ''} {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
            data-host-selected={selected}
            onclick={() => onSelectHost(originalIndex)}
            ondblclick={() => handleConnectHost(conn)}
            role="button"
            tabindex="-1"
            onkeydown={() => {}}
          >
            <div class="flex items-start justify-between gap-2">
              <div class="flex min-w-0 flex-col gap-1">
                <div class="flex min-w-0 items-center gap-2">
                  <span class="status-dot {status.cls}" title={status.label}></span>
                  <span class="truncate font-medium text-primary" title={conn.name}>{conn.name}</span>
                </div>
                <span class="truncate font-mono text-xs text-muted" title={`${conn.user}@${conn.host}:${conn.port}`}>
                  {conn.user}@{conn.host}{conn.port !== 22 ? `:${conn.port}` : ""}
                </span>
              </div>
              <div class="flex shrink-0 gap-1">{@render hostBadges(conn)}</div>
            </div>

            {#if conn.tags.length > 0}
              <div class="flex flex-wrap gap-1">
                {#each conn.tags.slice(0, 4) as tag (tag)}
                  <span class="tag">{tag}</span>
                {/each}
                {#if conn.tags.length > 4}
                  <span class="tag" title={conn.tags.slice(4).join(", ")}>+{conn.tags.length - 4}</span>
                {/if}
              </div>
            {/if}

            <div class="mt-auto flex items-center justify-between gap-2">
              <span class="text-xs text-muted" title={conn.last_used ? formatDateTime(conn.last_used, timezone) : undefined}>
                {lastUsedLabel(conn)}
              </span>
              <div class="flex items-center gap-1">
                <div class="row-actions">{@render hostActions(conn)}</div>
                {@render connectButton(conn)}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  /* Connect turns into the primary action on the row the user is on. */
  :global(tr:hover) .connect-btn,
  :global(tr.is-selected) .connect-btn,
  :global(.group:hover) .connect-btn {
    background: var(--color-accent-strong);
    border-color: transparent;
    color: var(--color-on-accent);
  }
</style>
