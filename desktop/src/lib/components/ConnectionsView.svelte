<script lang="ts">
  import {
    Activity,
    ArrowUpDown,
    Check,
    Copy,
    CopyPlus,
    Edit2,
    LayoutGrid,
    List,
    Play,
    Plus,
    RefreshCw,
    Search,
    Server,
    Terminal,
    Trash2,
    X,
  } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Connection } from "$lib/types";
  import { toSshCommand } from "$lib/utils/sshCommand";
  import { formatDate } from "$lib/utils/timezone";
  import { notify } from "$lib/stores/notifications.svelte";

  interface Props {
    connections: Connection[];
    viewMode: "list" | "grid";
    selectedHostIndex: number;
    copiedId: string | null;
    justDuplicatedId: string | null;
    timezone: string;
    searchQuery?: string;
    selectedTag?: string | null;
    onSelectHost: (index: number) => void;
    onConnect: (conn: Connection) => void;
    onEdit: (conn: Connection) => void;
    onDelete: (conn: Connection) => void;
    onDuplicate: (conn: Connection) => void;
    onCopyCommand: (text: string, id: string) => void;
    onRefresh: () => void;
    onAddHost: () => void;
    onOpenBatchExec?: () => void;
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
    onSelectHost,
    onConnect,
    onEdit,
    onDelete,
    onDuplicate,
    onCopyCommand,
    onRefresh,
    onAddHost,
    onOpenBatchExec,
  }: Props = $props();

  let pinging = $state(false);
  let pingResults = $state<Record<string, { latency_ms: number; success: boolean }>>({});
  let connectingHostId = $state<string | null>(null);
  let sortBy = $state<"bayesian" | "name" | "recent" | "host">("bayesian");

  type DisplayConnection = {
    conn: Connection;
    originalIndex: number;
  };

  async function handleConnectHost(conn: Connection) {
    connectingHostId = conn.id;
    try {
      await onConnect(conn);
    } finally {
      setTimeout(() => (connectingHostId = null), 1000);
    }
  }

  async function pingAllHosts() {
    pinging = true;
    try {
      const results = await invoke<Array<{ connection_id: string; success: boolean; latency_ms: number }>>("ping_all_connections");
      const map: Record<string, { latency_ms: number; success: boolean }> = {};
      let reachable = 0;
      for (const r of results) {
        map[r.connection_id] = { latency_ms: r.latency_ms, success: r.success };
        if (r.success) reachable++;
      }
      pingResults = map;
      notify(`Ping completed: ${reachable}/${results.length} reachable hosts`, "success");
    } catch (err) {
      notify(`Ping failed: ${err}`, "error");
    } finally {
      pinging = false;
    }
  }

  const tags = $derived.by(() => {
    const s = new Set<string>();
    connections.forEach((c) => c.tags.forEach((t) => s.add(t)));
    return Array.from(s).sort();
  });

  const processedConnections = $derived.by(() => {
    let list: DisplayConnection[] = connections.map((conn, originalIndex) => ({ conn, originalIndex }));

    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      list = list.filter(
        ({ conn }) =>
          conn.name.toLowerCase().includes(q) ||
          conn.host.toLowerCase().includes(q) ||
          conn.user.toLowerCase().includes(q) ||
          String(conn.port).includes(q) ||
          conn.tags.some((t) => t.toLowerCase().includes(q)),
      );
    }

    if (selectedTag) {
      list = list.filter(({ conn }) => conn.tags.includes(selectedTag!));
    }

    if (sortBy === "name") {
      list.sort((a, b) => a.conn.name.localeCompare(b.conn.name));
    } else if (sortBy === "recent") {
      list.sort((a, b) => (b.conn.last_used ?? "").localeCompare(a.conn.last_used ?? ""));
    } else if (sortBy === "host") {
      list.sort((a, b) => a.conn.host.localeCompare(b.conn.host));
    }

    return list;
  });

  const reachableCount = $derived.by(() => {
    return Object.values(pingResults).filter((r) => r.success).length;
  });

  const hasFilters = $derived(Boolean(searchQuery.trim() || selectedTag));

  function rowStatusClass(conn: Connection) {
    const ping = pingResults[conn.id];
    if (ping) return ping.success ? "status-dot-running" : "status-dot-error";
    return conn.last_used ? "status-dot-success" : "status-dot-offline";
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
    return processedConnections.findIndex(({ originalIndex }) => originalIndex === selectedHostIndex);
  }

  function handleVisibleHostKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || processedConnections.length === 0 || shouldIgnoreHostShortcuts()) return;

    const currentIndex = selectedDisplayIndex();

    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const fallbackIndex = e.key === "ArrowDown" ? 0 : processedConnections.length - 1;
      const nextIndex =
        currentIndex === -1
          ? fallbackIndex
          : e.key === "ArrowDown"
            ? (currentIndex + 1) % processedConnections.length
            : (currentIndex - 1 + processedConnections.length) % processedConnections.length;
      onSelectHost(processedConnections[nextIndex].originalIndex);
      return;
    }

    if (currentIndex === -1) return;
    const selected = processedConnections[currentIndex].conn;

    if (e.key === "Enter") {
      e.preventDefault();
      onConnect(selected);
      return;
    }

    if (e.key.toLowerCase() === "e" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      onEdit(selected);
    }
  }
</script>

<svelte:window onkeydown={handleVisibleHostKeydown} />

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden bg-surface select-none">
  <div class="toolbar-band gap-2 border-b border-border/80">
    <div class="flex min-w-[180px] flex-1 items-center gap-2">
      <div class="search-box h-8 w-full max-w-md">
        <Search size={14} class="text-muted shrink-0" />
        <input
          type="text"
          placeholder="Search hosts, users, addresses, ports, tags"
          bind:value={searchQuery}
          class="search-input w-full border-none bg-transparent text-[13px] text-primary outline-none placeholder:text-muted"
        />
        {#if searchQuery}
          <button
            type="button"
            onclick={() => (searchQuery = "")}
            class="btn-icon p-0.5"
            title="Clear search"
            aria-label="Clear search"
          >
            <X size={12} />
          </button>
        {/if}
      </div>

      {#if tags.length > 0}
        <div class="hidden lg:flex items-center gap-1 overflow-x-auto max-w-md scrollbar-none">
          <button
            type="button"
            class="filter-chip
              {selectedTag === null
                ? 'filter-chip-active'
                : 'filter-chip-idle'}"
            onclick={() => (selectedTag = null)}
          >
            All
          </button>
          {#each tags.slice(0, 8) as t}
            <button
              type="button"
              class="filter-chip whitespace-nowrap
                {selectedTag === t
                  ? 'filter-chip-active'
                  : 'filter-chip-idle'}"
              onclick={() => (selectedTag = selectedTag === t ? null : t)}
              title={`#${t}`}
            >
              #{t}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="flex shrink-0 items-center gap-2">
      <div class="flex h-8 items-center gap-1 rounded-md border border-border bg-surface-input px-2 text-xs">
        <ArrowUpDown size={12} class="text-muted" />
        <select
          bind:value={sortBy}
          class="h-6 border-none bg-transparent text-xs text-secondary outline-none cursor-pointer"
          title="Sort hosts"
          aria-label="Sort hosts"
        >
          <option value="bayesian" class="bg-surface text-primary">Bayesian</option>
          <option value="name" class="bg-surface text-primary">Name</option>
          <option value="recent" class="bg-surface text-primary">Most Recent</option>
          <option value="host" class="bg-surface text-primary">Host</option>
        </select>
      </div>

      <div class="segmented-control h-8">
        <button
          type="button"
          class="segmented-button
            {viewMode === 'list' ? 'segmented-button-active' : 'bg-transparent'}"
          onclick={() => (viewMode = "list")}
          title="Table List View"
          aria-label="Table List View"
        >
          <List size={14} />
        </button>
        <button
          type="button"
          class="segmented-button
            {viewMode === 'grid' ? 'segmented-button-active' : 'bg-transparent'}"
          onclick={() => (viewMode = "grid")}
          title="Grid Cards View"
          aria-label="Grid Cards View"
        >
          <LayoutGrid size={14} />
        </button>
      </div>

      <button
        type="button"
        class="toolbar-btn h-8"
        onclick={pingAllHosts}
        disabled={pinging}
        title="Ping all saved hosts"
      >
        <Activity size={13} class={pinging ? "animate-spin text-accent" : "text-success"} />
        <span class="hidden sm:inline">
          {#if Object.keys(pingResults).length > 0}
            Ping {reachableCount}/{connections.length}
          {:else}
            Ping
          {/if}
        </span>
      </button>

      {#if onOpenBatchExec}
        <button
          type="button"
          class="toolbar-btn h-8"
          onclick={onOpenBatchExec}
          title="Safe Multi-Host Batch Execution"
        >
          <Terminal size={13} class="text-secondary" />
          <span class="hidden sm:inline">Batch</span>
        </button>
      {/if}

      <button
        type="button"
        class="btn btn-primary h-8 shadow-sm"
        onclick={onAddHost}
      >
        <Plus size={14} />
        <span>Add Host</span>
      </button>
    </div>
  </div>

  <div class="view-content">
    {#if processedConnections.length > 0}
      {#if viewMode === "list"}
        <div class="overflow-hidden rounded-lg border border-border bg-surface-input/20">
          <table class="w-full table-fixed border-collapse text-xs">
            <thead class="bg-surface-input/60 text-2xs font-semibold uppercase tracking-wider text-muted select-none">
              <tr class="border-b border-border">
                <th class="w-10 px-3 py-2 text-left font-semibold">Rank</th>
                <th class="w-[30%] px-3 py-2 text-left font-semibold">Host</th>
                <th class="w-[32%] px-3 py-2 text-left font-semibold">Target</th>
                <th class="hidden px-3 py-2 text-left font-semibold xl:table-cell">Tags</th>
                <th class="hidden w-[14%] px-3 py-2 text-left font-semibold lg:table-cell">Last Used</th>
                <th class="w-[170px] sm:w-[200px] px-3 py-2 text-right font-semibold">Actions</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-border/50">
              {#each processedConnections as { conn, originalIndex }, index}
                {@const ping = pingResults[conn.id]}
                <tr
                  class="group cursor-pointer text-secondary outline-none transition-colors hover:bg-surface-hover/60 hover:text-primary
                    {selectedHostIndex === originalIndex ? 'bg-white/[0.04] text-primary' : ''}
                  {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
                  onclick={() => onSelectHost(originalIndex)}
                  ondblclick={() => onConnect(conn)}
                  tabindex="0"
                  aria-selected={selectedHostIndex === originalIndex}
                  onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
                >
                  <td class="px-3 py-2 font-mono text-[11px] text-muted/80">
                    #{sortBy === "bayesian" ? originalIndex + 1 : index + 1}
                  </td>
                  <td class="px-3 py-2">
                    <div class="flex min-w-0 items-center gap-2 font-medium text-primary">
                      <span
                        class="status-dot {rowStatusClass(conn)}"
                        title={ping ? (ping.success ? `Reachable: ${ping.latency_ms}ms` : "Host unreachable") : "Not pinged"}
                      ></span>
                      <span class="truncate" title={conn.name}>{conn.name}</span>
                      {#if conn.use_kerberos}
                        <span class="flag" title="Kerberos GSSAPI">krb5</span>
                      {/if}
                      {#if conn.bastion}
                        <span class="flag" title={`Jump host: ${conn.bastion}`}>jump</span>
                      {/if}
                    </div>
                  </td>
                  <td class="truncate px-3 py-2 font-mono text-2xs text-muted" title={`${conn.user}@${conn.host}:${conn.port}`}>
                    {conn.user}@{conn.host}:{conn.port}
                  </td>
                  <td class="hidden px-3 py-2 xl:table-cell">
                    <div class="flex flex-wrap gap-1">
                      {#each conn.tags.slice(0, 4) as tag}
                        <span class="tag" title={`#${tag}`}>#{tag}</span>
                      {/each}
                      {#if conn.tags.length > 4}
                        <span class="tag" title={conn.tags.slice(4).map(t => `#${t}`).join(", ")}>+{conn.tags.length - 4}</span>
                      {/if}
                    </div>
                  </td>
                  <td class="hidden truncate px-3 py-2 text-2xs text-muted lg:table-cell">
                    {conn.last_used ? formatDate(conn.last_used, timezone) : "Never"}
                  </td>
                  <td class="px-3 py-2">
                    <div class="flex items-center justify-end gap-1">
                      {#if ping?.success}
                        <span class="mr-1 hidden font-mono text-[10px] text-success sm:inline">{ping.latency_ms}ms</span>
                      {/if}
                      <div class="flex items-center gap-0.5 opacity-60 group-hover:opacity-100 transition-opacity">
                        <button
                          type="button"
                          class="btn-icon h-6 w-6 p-0"
                          onclick={(e) => {
                            e.stopPropagation();
                            onCopyCommand(toSshCommand(conn), conn.id);
                          }}
                          title="Copy SSH Command"
                          aria-label="Copy SSH Command"
                        >
                          {#if copiedId === conn.id}
                            <Check size={12} class="text-success" />
                          {:else}
                            <Copy size={12} />
                          {/if}
                        </button>
                        <button
                          type="button"
                          class="btn-icon h-6 w-6 p-0"
                          onclick={(e) => {
                            e.stopPropagation();
                            onEdit(conn);
                          }}
                          title="Edit Connection"
                          aria-label="Edit Connection"
                        >
                          <Edit2 size={12} />
                        </button>
                        <button
                          type="button"
                          class="btn-icon h-6 w-6 p-0"
                          onclick={(e) => {
                            e.stopPropagation();
                            onDuplicate(conn);
                          }}
                          title="Duplicate Connection"
                          aria-label="Duplicate Connection"
                        >
                          <CopyPlus size={12} />
                        </button>
                        <button
                          type="button"
                          class="btn-icon h-6 w-6 p-0 hover:text-error"
                          onclick={(e) => {
                            e.stopPropagation();
                            onDelete(conn);
                          }}
                          title="Delete Connection"
                          aria-label="Delete Connection"
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                      <button
                        type="button"
                        class="btn btn-primary btn-sm ml-1 h-6 px-2 text-2xs"
                        onclick={(e) => {
                          e.stopPropagation();
                          handleConnectHost(conn);
                        }}
                        disabled={connectingHostId !== null}
                        title={`Connect to ${conn.name}`}
                      >
                        {#if connectingHostId === conn.id}
                          <RefreshCw size={10} class="animate-spin" />
                          <span class="hidden sm:inline">Connecting</span>
                        {:else}
                          <Play size={10} fill="currentColor" />
                          <span class="hidden sm:inline">Connect</span>
                        {/if}
                      </button>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <div class="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-2.5">
          {#each processedConnections as { conn, originalIndex }}
            {@const ping = pingResults[conn.id]}
            <div
              class="server-card relative group
                {selectedHostIndex === originalIndex ? 'border-border-hover bg-surface-hover/70' : ''}"
              onclick={() => onSelectHost(originalIndex)}
              ondblclick={() => onConnect(conn)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
            >
              <div>
                <div class="flex items-center justify-between gap-2 mb-1.5">
                  <div class="flex items-center gap-2 min-w-0">
                    <span
                      class="status-dot {rowStatusClass(conn)}"
                      title={ping ? (ping.success ? `Reachable: ${ping.latency_ms}ms` : "Host unreachable") : "Not pinged"}
                    ></span>
                    <span class="font-semibold text-xs text-primary truncate" title={conn.name}>{conn.name}</span>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    {#if conn.use_kerberos}
                      <span class="flag" title="Kerberos GSSAPI">krb5</span>
                    {/if}
                    {#if conn.bastion}
                      <span class="flag" title={`Jump host: ${conn.bastion}`}>jump</span>
                    {/if}
                    {#if ping?.success}
                      <span class="font-mono text-[10px] text-success">
                        {ping.latency_ms}ms
                      </span>
                    {/if}
                  </div>
                </div>

                <div class="font-mono text-2xs text-muted truncate mb-2" title={`${conn.user}@${conn.host}:${conn.port}`}>
                  {conn.user}@{conn.host}:{conn.port}
                </div>

                <!-- Tags -->
                {#if conn.tags.length > 0}
                  <div class="flex flex-wrap gap-1 mb-2 max-h-12 overflow-hidden">
                    {#each conn.tags.slice(0, 5) as tag}
                      <span class="tag" title={`#${tag}`}>#{tag}</span>
                    {/each}
                    {#if conn.tags.length > 5}
                      <span class="tag" title={conn.tags.slice(5).map((t) => `#${t}`).join(", ")}>+{conn.tags.length - 5}</span>
                    {/if}
                  </div>
                {/if}
              </div>

              <div class="flex items-center justify-between pt-2 border-t border-border/60 mt-1">
                <span class="truncate text-[10px] text-muted">
                  {conn.last_used ? formatDate(conn.last_used, timezone) : "Never"}
                </span>

                <div class="flex shrink-0 items-center gap-0.5">
                  <button
                    type="button"
                    class="btn-icon h-6 w-6 p-0"
                    onclick={(e) => {
                      e.stopPropagation();
                      onCopyCommand(toSshCommand(conn), conn.id);
                    }}
                    title="Copy SSH Command"
                    aria-label="Copy SSH Command"
                  >
                    {#if copiedId === conn.id}
                      <Check size={12} class="text-success" />
                    {:else}
                      <Copy size={12} />
                    {/if}
                  </button>
                  <button
                    type="button"
                    class="btn-icon h-6 w-6 p-0"
                    onclick={(e) => {
                      e.stopPropagation();
                      onEdit(conn);
                    }}
                    title="Edit"
                    aria-label="Edit"
                  >
                    <Edit2 size={11} />
                  </button>
                  <button
                    type="button"
                    class="btn-icon h-6 w-6 p-0"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDuplicate(conn);
                    }}
                    title="Duplicate"
                    aria-label="Duplicate"
                  >
                    <CopyPlus size={11} />
                  </button>
                  <button
                    type="button"
                    class="btn-icon h-6 w-6 p-0 hover:text-error"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDelete(conn);
                    }}
                    title="Delete"
                    aria-label="Delete"
                  >
                    <Trash2 size={11} />
                  </button>
                  <button
                    type="button"
                    class="btn btn-primary btn-sm ml-1 h-6 px-2 text-2xs"
                    onclick={(e) => {
                      e.stopPropagation();
                      handleConnectHost(conn);
                    }}
                    disabled={connectingHostId !== null}
                    title={`Connect to ${conn.name}`}
                  >
                    {#if connectingHostId === conn.id}
                      <RefreshCw size={10} class="animate-spin" />
                      <span class="hidden sm:inline">Connecting</span>
                    {:else}
                      <Play size={9} fill="currentColor" />
                      <span class="hidden sm:inline">Connect</span>
                    {/if}
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      <div class="flex flex-col items-center justify-center py-20 text-muted border border-dashed border-border rounded-lg bg-surface-input/10">
        <div class="w-12 h-12 rounded-lg bg-surface-input border border-border flex items-center justify-center text-accent mb-3">
          <Server size={24} />
        </div>
        <h3 class="text-sm font-bold text-primary mb-1">
          {hasFilters ? "No matching SSH hosts" : "No SSH hosts configured"}
        </h3>
        <p class="text-xs text-muted max-w-sm text-center mb-4 leading-relaxed">
          {hasFilters
            ? "Clear the current search or tag filter to return to the full host list."
            : "Add your first remote host or import existing connections from your OpenSSH configuration."}
        </p>
        {#if hasFilters}
          <button
            type="button"
            class="btn btn-secondary"
            onclick={() => {
              searchQuery = "";
              selectedTag = null;
            }}
          >
            Clear Filters
          </button>
        {:else}
          <button
            type="button"
            class="btn btn-primary"
            onclick={onAddHost}
          >
            <Plus size={14} />
            <span>Add First Connection</span>
          </button>
        {/if}
      </div>
    {/if}
  </div>
</div>
