<script lang="ts">
  import {
    Plus,
    Trash2,
    Edit2,
    Play,
    Server,
    Shield,
    Copy,
    Check,
    RefreshCw,
    CopyPlus,
    Terminal,
    Activity,
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
    viewMode,
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
      notify(`Ping completed: ${reachable}/${results.length} hosts reachable!`, "success");
    } catch (err) {
      notify(`Ping failed: ${err}`, "error");
    } finally {
      pinging = false;
    }
  }
</script>

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden bg-surface">
  <!-- Header -->
  <div class="shrink-0 px-6 pt-5 pb-3 flex justify-between items-center border-b border-border">
    <div>
      <h2 class="text-base font-bold tracking-tight text-primary flex items-center gap-2 m-0">
        <Server size={18} class="text-accent" />
        SSH Connections
        <span class="text-xs font-semibold px-2 py-0.5 rounded-full bg-accent/15 text-accent border border-accent/20">
          {connections.length}
        </span>
      </h2>
      <span class="text-muted text-xs mt-0.5 block">Bayesian-ranked hosts based on frequency and recency</span>
    </div>

    <div class="flex items-center gap-2">
      {#if onOpenBatchExec}
        <button
          class="bg-surface-input border border-border text-muted px-2.5 py-1.5 rounded-md cursor-pointer flex items-center gap-1.5 text-xs font-semibold transition-all hover:border-border-hover hover:text-primary disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={onOpenBatchExec}
          title="Safe Multi-Host Batch Execution"
        >
          <Terminal size={14} class="text-accent" />
          Batch Exec
        </button>
      {/if}
      <button
        class="bg-surface-input border border-border text-muted px-2.5 py-1.5 rounded-md cursor-pointer flex items-center gap-1.5 text-xs font-semibold transition-all hover:border-border-hover hover:text-primary disabled:opacity-50 disabled:cursor-not-allowed"
        onclick={pingAllHosts}
        disabled={pinging}
        title="Ping all hosts"
      >
        <Activity size={14} class={pinging ? "animate-spin text-accent" : "text-running"} />
        Ping All
      </button>
      <button
        class="bg-surface-input border border-border text-muted p-1.5 rounded-md cursor-pointer flex transition-all hover:border-border-hover hover:text-primary disabled:opacity-50 disabled:cursor-not-allowed"
        onclick={onRefresh}
        title="Refresh Connections"
      >
        <RefreshCw size={14} />
      </button>
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-accent text-white text-xs font-semibold cursor-pointer shadow-sm hover:opacity-90 transition-all disabled:opacity-50 disabled:cursor-not-allowed"
        onclick={onAddHost}
      >
        <Plus size={14} />
        Add Host
      </button>
    </div>
  </div>

  <!-- Content -->
  <div class="flex-1 min-h-0 overflow-y-auto px-6 py-4 scrollbar-none">
    {#if connections.length > 0}
      {#if viewMode === "list"}
        <div class="border border-border rounded-lg overflow-hidden bg-surface-input/30">
          <div class="flex bg-surface-input border-b border-border text-[10px] font-bold text-muted uppercase tracking-wider px-3.5 py-2">
            <div class="flex-[2]">Name</div>
            <div class="flex-[2.5]">Target Address</div>
            <div class="flex-[2]">Tags</div>
            <div class="flex-[1.5]">Last Used</div>
            <div class="flex-[1.5] text-right">Actions</div>
          </div>

          <div class="divide-y divide-border/60">
            {#each connections as conn, index}
              <div
                class="flex items-center px-3.5 py-2 text-xs text-secondary cursor-pointer outline-none transition-colors hover:bg-white/[0.04] hover:text-primary group
                  {selectedHostIndex === index ? 'bg-accent/10 text-primary font-semibold' : ''}
                  {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
                onclick={() => onSelectHost(index)}
                ondblclick={() => onConnect(conn)}
                role="row"
                tabindex="0"
                onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
              >
                <div class="flex-[2] flex items-center gap-2 font-medium text-primary">
                  <span class="w-2 h-2 rounded-full {conn.last_used ? 'bg-running' : 'bg-muted/40'} shrink-0"></span>
                  <span class="truncate">{conn.name}</span>
                  {#if conn.use_kerberos}
                    <span class="text-[9px] px-1 py-0.2 rounded bg-accent/15 border border-accent/25 text-accent font-bold uppercase tracking-wider">krb5</span>
                  {/if}
                  {#if conn.bastion}
                    <span class="text-[9px] px-1 py-0.2 rounded bg-cyan-500/15 border border-cyan-500/25 text-cyan-400 font-bold uppercase tracking-wider">jump</span>
                  {/if}
                </div>

                <div class="flex-[2.5] font-mono text-[11px] text-muted truncate">{conn.user}@{conn.host}:{conn.port}</div>

                <div class="flex-[2] flex gap-1 flex-wrap">
                  {#each conn.tags as tag}
                    <span class="text-[10px] bg-white/[0.04] border border-border text-muted px-1.5 py-0.2 rounded font-mono">#{tag}</span>
                  {/each}
                </div>

                <div class="flex-[1.5] text-muted text-[11px]">
                  {conn.last_used ? formatDate(conn.last_used, timezone) : "Never"}
                </div>

                <div class="flex-[1.5] flex justify-end gap-1 opacity-60 group-hover:opacity-100 transition-opacity">
                  <button
                    class="p-1 rounded text-muted hover:text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    onclick={(e) => {
                      e.stopPropagation();
                      onCopyCommand(toSshCommand(conn), conn.id);
                    }}
                    title="Copy SSH command"
                  >
                    {#if copiedId === conn.id}
                      <Check size={13} class="text-running" />
                    {:else}
                      <Copy size={13} />
                    {/if}
                  </button>
                  <button
                    class="p-1 rounded text-muted hover:text-primary hover:bg-white/10 transition-colors"
                    onclick={(e) => {
                      e.stopPropagation();
                      onEdit(conn);
                    }}
                    title="Edit"
                  >
                    <Edit2 size={13} />
                  </button>
                  <button
                    class="p-1 rounded text-muted hover:text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDuplicate(conn);
                    }}
                    title="Duplicate"
                  >
                    <CopyPlus size={13} />
                  </button>
                  <button
                    class="p-1 rounded text-muted hover:text-error hover:bg-error/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDelete(conn);
                    }}
                    title="Delete"
                  >
                    <Trash2 size={13} />
                  </button>
                  <button
                    class="p-1 px-2 rounded bg-accent/15 text-accent font-semibold hover:bg-accent hover:text-white transition-all flex items-center gap-1 text-[11px] disabled:opacity-50 disabled:cursor-not-allowed"
                    onclick={(e) => {
                      e.stopPropagation();
                      handleConnectHost(conn);
                    }}
                    disabled={connectingHostId !== null}
                    title="Connect"
                  >
                    {#if connectingHostId === conn.id}
                      <RefreshCw size={11} class="animate-spin" />
                      <span>Connecting...</span>
                    {:else}
                      <Play size={11} fill="currentColor" />
                      <span>Connect</span>
                    {/if}
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if viewMode === "grid"}
        <div class="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3">
          {#each connections as conn, index}
            <div
              class="bg-surface-input/50 border border-border rounded-lg p-3.5 flex flex-col justify-between transition-all hover:border-border-hover hover:bg-surface-input group relative
                {selectedHostIndex === index ? 'border-accent bg-accent/5' : ''}"
              onclick={() => onSelectHost(index)}
              ondblclick={() => onConnect(conn)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
            >
              <div>
                <div class="flex items-center justify-between mb-2">
                  <div class="flex items-center gap-2 min-w-0">
                    <span class="w-2 h-2 rounded-full {conn.last_used ? 'bg-running' : 'bg-muted/40'} shrink-0"></span>
                    <span class="font-bold text-xs text-primary truncate">{conn.name}</span>
                  </div>
                  <div class="flex items-center gap-1">
                    {#if conn.use_kerberos}
                      <span class="text-[9px] px-1 py-0.2 rounded bg-accent/15 border border-accent/25 text-accent font-bold">krb5</span>
                    {/if}
                    {#if conn.bastion}
                      <span class="text-[9px] px-1 py-0.2 rounded bg-cyan-500/15 border border-cyan-500/25 text-cyan-400 font-bold">jump</span>
                    {/if}
                  </div>
                </div>

                <div class="font-mono text-[11px] text-muted truncate mb-2">{conn.user}@{conn.host}:{conn.port}</div>

                {#if conn.tags.length > 0}
                  <div class="flex flex-wrap gap-1 mb-3">
                    {#each conn.tags as tag}
                      <span class="text-[10px] bg-white/[0.04] border border-border text-muted px-1.5 py-0.2 rounded font-mono">#{tag}</span>
                    {/each}
                  </div>
                {/if}
              </div>

              <div class="flex items-center justify-between pt-2 border-t border-white/5 mt-2">
                <span class="text-[10px] text-muted">
                  {conn.last_used ? formatDate(conn.last_used, timezone) : "Never"}
                </span>
                <div class="flex items-center gap-1">
                  <button
                    class="p-1 rounded text-muted hover:text-primary hover:bg-white/10 transition-colors"
                    onclick={(e) => {
                      e.stopPropagation();
                      onEdit(conn);
                    }}
                    title="Edit"
                  >
                    <Edit2 size={12} />
                  </button>
                  <button
                    class="px-2 py-1 rounded bg-accent text-white text-[11px] font-semibold hover:opacity-90 transition-all flex items-center gap-1 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                    onclick={(e) => {
                      e.stopPropagation();
                      handleConnectHost(conn);
                    }}
                    disabled={connectingHostId !== null}
                  >
                    {#if connectingHostId === conn.id}
                      <RefreshCw size={10} class="animate-spin" />
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
      <div class="flex flex-col items-center justify-center py-20 text-muted border border-dashed border-border rounded-xl">
        <Server size={36} class="mb-3 opacity-40 text-accent" />
        <span class="text-sm font-semibold text-primary mb-1">No SSH Connections</span>
        <span class="text-xs max-w-sm text-center mb-4">Add your first remote host or import connections from OpenSSH config.</span>
        <button
          class="flex items-center gap-1.5 px-3.5 py-2 rounded-md bg-accent text-white text-xs font-semibold cursor-pointer shadow-sm hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={onAddHost}
        >
          <Plus size={14} />
          Add Connection
        </button>
      </div>
    {/if}
  </div>
</div>
