<script lang="ts">
  import {
    Activity,
    ArrowUpDown,
    Check,
    CheckCircle2,
    Copy,
    CopyPlus,
    Edit2,
    HardDrive,
    LayoutGrid,
    List,
    Play,
    Plus,
    RefreshCw,
    Search,
    Server,
    Shield,
    Terminal,
    Trash2,
    X,
    Zap,
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
  let filterQuery = $state("");
  let selectedTagFilter = $state<string | null>(null);

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

  // Extract all unique tags
  const tags = $derived.by(() => {
    const s = new Set<string>();
    connections.forEach((c) => c.tags.forEach((t) => s.add(t)));
    return Array.from(s).sort();
  });

  // Filtered & Sorted connections
  const processedConnections = $derived.by(() => {
    let list = [...connections];

    if (filterQuery.trim()) {
      const q = filterQuery.toLowerCase();
      list = list.filter(
        (c) =>
          c.name.toLowerCase().includes(q) ||
          c.host.toLowerCase().includes(q) ||
          c.user.toLowerCase().includes(q) ||
          c.tags.some((t) => t.toLowerCase().includes(q)),
      );
    }

    if (selectedTagFilter) {
      list = list.filter((c) => c.tags.includes(selectedTagFilter!));
    }

    if (sortBy === "name") {
      list.sort((a, b) => a.name.localeCompare(b.name));
    } else if (sortBy === "recent") {
      list.sort((a, b) => (b.last_used ?? "").localeCompare(a.last_used ?? ""));
    } else if (sortBy === "host") {
      list.sort((a, b) => a.host.localeCompare(b.host));
    }
    // "bayesian" keeps the ranking returned from the bayesian-ssh engine

    return list;
  });

  // Bayesian top host
  const topHost = $derived.by(() => {
    return connections.length > 0 ? connections[0] : null;
  });

  const reachableCount = $derived.by(() => {
    return Object.values(pingResults).filter((r) => r.success).length;
  });
</script>

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden bg-surface select-none">
  <!-- Top Executive Metric Bar -->
  <div class="grid grid-cols-2 md:grid-cols-4 gap-3 px-6 pt-5 pb-3 shrink-0">
    <div class="metric-tile">
      <span class="eyebrow flex items-center justify-between">
        Total Hosts
        <Server size={12} class="text-accent" />
      </span>
      <div class="flex items-baseline gap-2 mt-0.5">
        <span class="text-xl font-bold tracking-tight text-primary">{connections.length}</span>
        <span class="text-[11px] text-muted">configured</span>
      </div>
    </div>

    <div class="metric-tile">
      <span class="eyebrow flex items-center justify-between">
        Reachable
        <Activity size={12} class="text-running" />
      </span>
      <div class="flex items-baseline gap-2 mt-0.5">
        <span class="text-xl font-bold tracking-tight text-running">
          {Object.keys(pingResults).length > 0 ? `${reachableCount}/${connections.length}` : '—'}
        </span>
        <span class="text-[11px] text-muted">live ping</span>
      </div>
    </div>

    <div class="metric-tile">
      <span class="eyebrow flex items-center justify-between">
        Bayesian Pick
        <Zap size={12} class="text-warning" />
      </span>
      <div class="flex items-baseline gap-1.5 mt-0.5 min-w-0">
        <span class="text-sm font-bold text-primary truncate max-w-[140px]">
          {topHost ? topHost.name : 'None'}
        </span>
        <span class="text-[10px] text-accent font-mono">Rank #1</span>
      </div>
    </div>

    <div class="metric-tile">
      <span class="eyebrow flex items-center justify-between">
        Tags Active
        <Shield size={12} class="text-secondary" />
      </span>
      <div class="flex items-baseline gap-2 mt-0.5">
        <span class="text-xl font-bold tracking-tight text-primary">{tags.length}</span>
        <span class="text-[11px] text-muted">categories</span>
      </div>
    </div>
  </div>

  <!-- Filter, Search & View Controls Bar -->
  <div class="px-6 py-2.5 flex items-center justify-between gap-3 border-y border-border/80 bg-surface-input/20 shrink-0 flex-wrap">
    <div class="flex items-center gap-2 flex-1 min-w-[260px]">
      <!-- Local search input -->
      <div class="relative flex items-center bg-surface-input border border-border rounded-lg px-2.5 py-1.5 w-full max-w-sm focus-within:border-accent">
        <Search size={14} class="text-muted mr-2 shrink-0" />
        <input
          type="text"
          placeholder="Filter hosts by name, host, port..."
          bind:value={filterQuery}
          class="bg-transparent border-none text-xs text-primary outline-none w-full placeholder:text-muted"
        />
        {#if filterQuery}
          <button
            type="button"
            onclick={() => (filterQuery = "")}
            class="text-muted hover:text-primary p-0.5 border-none bg-transparent cursor-pointer"
          >
            <X size={12} />
          </button>
        {/if}
      </div>

      <!-- Tag Quick Filter Pills -->
      {#if tags.length > 0}
        <div class="hidden lg:flex items-center gap-1 overflow-x-auto max-w-md scrollbar-none">
          <button
            type="button"
            class="text-[10px] px-2 py-1 rounded-md cursor-pointer transition-all border
              {selectedTagFilter === null
                ? 'border-accent/40 bg-accent/20 text-accent font-bold'
                : 'border-border bg-surface text-muted hover:text-primary hover:border-border-hover'}"
            onclick={() => (selectedTagFilter = null)}
          >
            All
          </button>
          {#each tags.slice(0, 5) as t}
            <button
              type="button"
              class="text-[10px] px-2 py-1 rounded-md cursor-pointer transition-all border whitespace-nowrap
                {selectedTagFilter === t
                  ? 'border-accent/40 bg-accent/20 text-accent font-bold'
                  : 'border-border bg-surface text-muted hover:text-primary hover:border-border-hover'}"
              onclick={() => (selectedTagFilter = selectedTagFilter === t ? null : t)}
            >
              #{t}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Actions & View Mode Toggle -->
    <div class="flex items-center gap-2">
      <!-- Sort Selector -->
      <div class="flex items-center gap-1 bg-surface-input border border-border rounded-lg px-2 py-1 text-xs">
        <ArrowUpDown size={12} class="text-muted" />
        <select
          bind:value={sortBy}
          class="bg-transparent border-none text-xs text-secondary outline-none cursor-pointer pr-1"
        >
          <option value="bayesian" class="bg-surface text-primary">Bayesian Rank</option>
          <option value="name" class="bg-surface text-primary">Name (A-Z)</option>
          <option value="recent" class="bg-surface text-primary">Most Recent</option>
          <option value="host" class="bg-surface text-primary">Host IP</option>
        </select>
      </div>

      <!-- View Switcher (List / Grid) -->
      <div class="flex border border-border rounded-lg p-0.5 bg-surface-input">
        <button
          type="button"
          class="p-1 rounded-md cursor-pointer border-none transition-colors
            {viewMode === 'list' ? 'bg-surface-hover text-primary shadow-sm' : 'bg-transparent text-muted hover:text-secondary'}"
          onclick={() => (viewMode = "list")}
          title="Table List View"
          aria-label="Table List View"
        >
          <List size={14} />
        </button>
        <button
          type="button"
          class="p-1 rounded-md cursor-pointer border-none transition-colors
            {viewMode === 'grid' ? 'bg-surface-hover text-primary shadow-sm' : 'bg-transparent text-muted hover:text-secondary'}"
          onclick={() => (viewMode = "grid")}
          title="Grid Cards View"
          aria-label="Grid Cards View"
        >
          <LayoutGrid size={14} />
        </button>
      </div>

      <!-- Ping All button -->
      <button
        type="button"
        class="bg-surface-input border border-border text-secondary hover:text-primary hover:border-border-hover px-2.5 py-1.5 rounded-lg cursor-pointer flex items-center gap-1.5 text-xs font-semibold transition-all"
        onclick={pingAllHosts}
        disabled={pinging}
        title="Ping all saved hosts"
      >
        <Activity size={13} class={pinging ? "animate-spin text-accent" : "text-running"} />
        <span class="hidden sm:inline">Ping All</span>
      </button>

      <!-- Batch Exec button -->
      {#if onOpenBatchExec}
        <button
          type="button"
          class="bg-surface-input border border-border text-secondary hover:text-primary hover:border-border-hover px-2.5 py-1.5 rounded-lg cursor-pointer flex items-center gap-1.5 text-xs font-semibold transition-all"
          onclick={onOpenBatchExec}
          title="Safe Multi-Host Batch Execution"
        >
          <Terminal size={13} class="text-accent" />
          <span class="hidden sm:inline">Batch</span>
        </button>
      {/if}

      <!-- New Server Primary Button -->
      <button
        type="button"
        class="btn btn-primary shadow-sm"
        onclick={onAddHost}
      >
        <Plus size={14} />
        <span>Add Host</span>
      </button>
    </div>
  </div>

  <!-- Main Content View Area -->
  <div class="flex-1 min-h-0 overflow-y-auto px-6 py-4 scrollbar-none">
    {#if processedConnections.length > 0}
      {#if viewMode === "list"}
        <!-- Modern Dense Table List -->
        <div class="border border-border rounded-xl overflow-hidden bg-surface-input/30 shadow-sm">
          <!-- Table Header -->
          <div class="flex items-center px-4 py-2.5 bg-surface-input/80 border-b border-border text-[10px] font-bold text-muted uppercase tracking-wider">
            <div class="w-12 text-center">Rank</div>
            <div class="flex-[3]">Host Name</div>
            <div class="flex-[3.5]">Target Address</div>
            <div class="flex-[2] hidden md:block">Tags</div>
            <div class="flex-[2] hidden lg:block">Last Used</div>
            <div class="flex-[2] text-right">Actions</div>
          </div>

          <!-- Table Rows -->
          <div class="divide-y divide-border/60">
            {#each processedConnections as conn, index}
              {@const ping = pingResults[conn.id]}
              <div
                class="flex items-center px-4 py-2.5 text-xs text-secondary cursor-pointer outline-none transition-all hover:bg-white/[0.04] hover:text-primary group
                  {selectedHostIndex === index ? 'bg-accent/10 text-primary font-semibold' : ''}
                  {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
                onclick={() => onSelectHost(index)}
                ondblclick={() => onConnect(conn)}
                role="row"
                tabindex="0"
                onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
              >
                <!-- Rank Index -->
                <div class="w-12 text-center font-mono text-[11px] text-muted">
                  #{index + 1}
                </div>

                <!-- Host Name & Badges -->
                <div class="flex-[3] flex items-center gap-2 font-semibold text-primary min-w-0">
                  <!-- Live Ping Indicator / Status Dot -->
                  {#if ping}
                    <span
                      class="w-2 h-2 rounded-full shrink-0 {ping.success ? 'bg-running' : 'bg-error'}"
                      title={ping.success ? `Reachable: ${ping.latency_ms}ms` : 'Host unreachable'}
                    ></span>
                  {:else}
                    <span class="w-2 h-2 rounded-full shrink-0 {conn.last_used ? 'bg-running/70' : 'bg-muted/40'}"></span>
                  {/if}

                  <span class="truncate">{conn.name}</span>

                  {#if conn.use_kerberos}
                    <span class="badge-pill bg-accent/15 border border-accent/30 text-accent text-[9px] uppercase">
                      krb5
                    </span>
                  {/if}
                  {#if conn.bastion}
                    <span class="badge-pill bg-cyan-500/15 border border-cyan-500/30 text-cyan-400 text-[9px] uppercase">
                      jump
                    </span>
                  {/if}
                  {#if ping?.success}
                    <span class="badge-pill bg-emerald-500/10 border border-emerald-500/25 text-emerald-400 font-mono text-[9px]">
                      {ping.latency_ms}ms
                    </span>
                  {/if}
                </div>

                <!-- Target Address -->
                <div class="flex-[3.5] font-mono text-[11px] text-muted truncate">
                  {conn.user}@{conn.host}:{conn.port}
                </div>

                <!-- Tags -->
                <div class="flex-[2] hidden md:flex gap-1 flex-wrap">
                  {#each conn.tags as tag}
                    <span class="tag">#{tag}</span>
                  {/each}
                </div>

                <!-- Last Used -->
                <div class="flex-[2] hidden lg:block text-muted text-[11px]">
                  {conn.last_used ? formatDate(conn.last_used, timezone) : "Never"}
                </div>

                <!-- Actions Hover Toolbar -->
                <div class="flex-[2] flex justify-end items-center gap-1 opacity-80 group-hover:opacity-100 transition-opacity">
                  <button
                    type="button"
                    class="btn-icon p-1 text-muted hover:text-primary transition-colors"
                    onclick={(e) => {
                      e.stopPropagation();
                      onCopyCommand(toSshCommand(conn), conn.id);
                    }}
                    title="Copy SSH Command"
                    aria-label="Copy SSH Command"
                  >
                    {#if copiedId === conn.id}
                      <Check size={13} class="text-running" />
                    {:else}
                      <Copy size={13} />
                    {/if}
                  </button>

                  <button
                    type="button"
                    class="btn-icon p-1 text-muted hover:text-primary transition-colors"
                    onclick={(e) => {
                      e.stopPropagation();
                      onEdit(conn);
                    }}
                    title="Edit Connection"
                    aria-label="Edit Connection"
                  >
                    <Edit2 size={13} />
                  </button>

                  <button
                    type="button"
                    class="btn-icon p-1 text-muted hover:text-primary transition-colors"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDuplicate(conn);
                    }}
                    title="Duplicate Connection"
                    aria-label="Duplicate Connection"
                  >
                    <CopyPlus size={13} />
                  </button>

                  <button
                    type="button"
                    class="btn-icon p-1 text-muted hover:text-error hover:bg-error/10 transition-colors"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDelete(conn);
                    }}
                    title="Delete Connection"
                    aria-label="Delete Connection"
                  >
                    <Trash2 size={13} />
                  </button>

                  <!-- Connect Primary Trigger -->
                  <button
                    type="button"
                    class="px-2.5 py-1 rounded-md bg-accent/15 border border-accent/30 text-accent font-semibold hover:bg-accent hover:text-white transition-all flex items-center gap-1 text-[11px] ml-1"
                    onclick={(e) => {
                      e.stopPropagation();
                      handleConnectHost(conn);
                    }}
                    disabled={connectingHostId !== null}
                    title="Open SSH Terminal"
                  >
                    {#if connectingHostId === conn.id}
                      <RefreshCw size={11} class="animate-spin" />
                      <span>Connecting...</span>
                    {:else}
                      <Play size={10} fill="currentColor" />
                      <span>Connect</span>
                    {/if}
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <!-- Modern Grid Cards View -->
        <div class="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3.5">
          {#each processedConnections as conn, index}
            {@const ping = pingResults[conn.id]}
            <div
              class="server-card relative group
                {selectedHostIndex === index ? 'border-accent bg-accent/5' : ''}"
              onclick={() => onSelectHost(index)}
              ondblclick={() => onConnect(conn)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
            >
              <div>
                <!-- Card Header -->
                <div class="flex items-center justify-between gap-2 mb-2">
                  <div class="flex items-center gap-2 min-w-0">
                    <span class="w-2.5 h-2.5 rounded-full shrink-0 {ping ? (ping.success ? 'bg-running' : 'bg-error') : (conn.last_used ? 'bg-running' : 'bg-muted/40')}"></span>
                    <span class="font-bold text-sm text-primary truncate">{conn.name}</span>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    {#if conn.use_kerberos}
                      <span class="badge-pill bg-accent/15 border border-accent/25 text-accent text-[9px]">krb5</span>
                    {/if}
                    {#if conn.bastion}
                      <span class="badge-pill bg-cyan-500/15 border border-cyan-500/25 text-cyan-400 text-[9px]">jump</span>
                    {/if}
                    {#if ping?.success}
                      <span class="badge-pill bg-emerald-500/15 text-emerald-400 font-mono text-[9px]">
                        {ping.latency_ms}ms
                      </span>
                    {/if}
                  </div>
                </div>

                <!-- Host Address -->
                <div class="font-mono text-xs text-muted truncate mb-2.5">
                  {conn.user}@{conn.host}:{conn.port}
                </div>

                <!-- Tags -->
                {#if conn.tags.length > 0}
                  <div class="flex flex-wrap gap-1 mb-3">
                    {#each conn.tags as tag}
                      <span class="tag">#{tag}</span>
                    {/each}
                  </div>
                {/if}
              </div>

              <!-- Card Footer Actions -->
              <div class="flex items-center justify-between pt-3 border-t border-border/60 mt-3">
                <span class="text-[10px] text-muted">
                  {conn.last_used ? formatDate(conn.last_used, timezone) : "Never used"}
                </span>

                <div class="flex items-center gap-1">
                  <button
                    type="button"
                    class="p-1 rounded text-muted hover:text-primary hover:bg-surface-hover transition-colors border-none bg-transparent cursor-pointer"
                    onclick={(e) => {
                      e.stopPropagation();
                      onEdit(conn);
                    }}
                    title="Edit"
                    aria-label="Edit"
                  >
                    <Edit2 size={12} />
                  </button>

                  <button
                    type="button"
                    class="px-2.5 py-1 rounded-md bg-accent text-white text-xs font-semibold hover:bg-accent-hover transition-all flex items-center gap-1.5 cursor-pointer border-none"
                    onclick={(e) => {
                      e.stopPropagation();
                      handleConnectHost(conn);
                    }}
                    disabled={connectingHostId !== null}
                  >
                    {#if connectingHostId === conn.id}
                      <RefreshCw size={11} class="animate-spin" />
                      <span>Connecting...</span>
                    {:else}
                      <Play size={10} fill="currentColor" />
                      <span>Connect</span>
                    {/if}
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      <!-- Empty State -->
      <div class="flex flex-col items-center justify-center py-20 text-muted border border-dashed border-border rounded-2xl bg-surface-input/10">
        <div class="w-12 h-12 rounded-2xl bg-accent/10 border border-accent/25 flex items-center justify-center text-accent mb-3">
          <Server size={24} />
        </div>
        <h3 class="text-sm font-bold text-primary mb-1">
          {filterQuery || selectedTagFilter ? "No matching SSH connections" : "No SSH Connections Configured"}
        </h3>
        <p class="text-xs text-muted max-w-sm text-center mb-4 leading-relaxed">
          {filterQuery || selectedTagFilter
            ? "Try changing your search query or tag filter."
            : "Add your first remote host or import existing connections from your OpenSSH configuration."}
        </p>
        {#if filterQuery || selectedTagFilter}
          <button
            type="button"
            class="btn btn-secondary"
            onclick={() => {
              filterQuery = "";
              selectedTagFilter = null;
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
