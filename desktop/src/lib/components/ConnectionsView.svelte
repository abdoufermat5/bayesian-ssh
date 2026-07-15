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
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { toSshCommand } from "$lib/utils/sshCommand";
  import { formatDate } from "$lib/utils/timezone";

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
  }: Props = $props();
</script>

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden">
  <div class="shrink-0 px-6 pt-5 pb-3 flex justify-between items-start">
    <div>
      <h2 class="text-lg m-0 font-semibold tracking-tight text-primary">SSH Connections</h2>
      <span class="text-muted text-xs mt-0.5 block">Bayesian ranked hosts based on connection frequency and recency</span>
    </div>
    <button
      class="bg-transparent border border-border text-muted p-1.5 rounded-lg cursor-pointer flex transition-all duration-100 hover:border-border-hover hover:text-primary"
      onclick={onRefresh}
      title="Refresh Connections"
    >
      <RefreshCw size={14} />
    </button>
  </div>

  <div class="flex-1 min-h-0 overflow-y-auto overflow-x-hidden px-6 pb-6 overscroll-contain">
    {#if connections.length > 0}
      {#if viewMode === "list"}
        <div class="border border-border rounded-xl overflow-hidden">
          <div class="flex bg-white/[0.02] border-b border-border text-[11px] font-semibold text-muted uppercase tracking-wider px-4 py-2.5">
            <div class="flex-[2]">Name</div>
            <div class="flex-[2.5]">Address</div>
            <div class="flex-[2]">Tags</div>
            <div class="flex-[1.5]">Last Session</div>
            <div class="flex-[2]"></div>
          </div>

          <div class="divide-y divide-border">
            {#each connections as conn, index}
              <div
                class="flex items-center px-4 py-2.5 text-[13px] text-secondary cursor-pointer outline-none transition-colors duration-100 hover:bg-white/[0.03] hover:text-primary group
                  {selectedHostIndex === index ? 'bg-accent/[0.06] text-primary' : ''}
                  {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
                onclick={() => onSelectHost(index)}
                ondblclick={() => onConnect(conn)}
                role="row"
                tabindex="0"
                onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
              >
                <div class="flex-[2] flex items-center gap-2 font-semibold text-primary">
                  <span class="inline-flex items-center justify-center w-4 h-4 shrink-0">
                    <Server size={14} class="text-muted" />
                  </span>
                  <span>{conn.name}</span>
                  {#if conn.use_kerberos}
                    <span class="text-[10px] px-1.5 py-px rounded-full bg-accent/10 border border-accent/20 text-accent font-semibold" title="Kerberos authentication enabled">krb5</span>
                  {/if}
                </div>

                <div class="flex-[2.5] font-mono">{conn.user}@{conn.host}:{conn.port}</div>

                <div class="flex-[2] flex gap-1 flex-wrap">
                  {#each conn.tags as tag}
                    <span class="text-[10px] bg-white/[0.04] border border-border text-muted px-1.5 py-px rounded-full">#{tag}</span>
                  {/each}
                </div>

                <div class="flex-[1.5] text-muted">
                  {conn.last_used ? formatDate(conn.last_used, timezone) : "Never"}
                </div>

                <div class="flex-[2] flex justify-end gap-1 opacity-40 transition-opacity duration-100 group-hover:opacity-100 focus-within:opacity-100">
                  <button
                    class="bg-transparent border border-transparent text-muted w-7 h-7 p-0 rounded-md cursor-pointer inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:bg-white/[0.06] hover:text-primary"
                    onclick={(e) => {
                      e.stopPropagation();
                      onCopyCommand(toSshCommand(conn), conn.id);
                    }}
                    title="Copy SSH command"
                  >
                    {#if copiedId === conn.id}
                      <Check size={14} />
                    {:else}
                      <Copy size={14} />
                    {/if}
                  </button>
                  <button
                    class="bg-transparent border border-transparent text-muted w-7 h-7 p-0 rounded-md cursor-pointer inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:bg-white/[0.06] hover:text-primary"
                    onclick={(e) => {
                      e.stopPropagation();
                      onEdit(conn);
                    }}
                    title="Edit"
                  >
                    <Edit2 size={14} />
                  </button>
                  <button
                    class="bg-transparent border border-transparent text-muted w-7 h-7 p-0 rounded-md cursor-pointer inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:bg-white/[0.06] hover:text-primary"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDuplicate(conn);
                    }}
                    title="Duplicate Connection"
                  >
                    <CopyPlus size={14} />
                  </button>
                  <button
                    class="bg-transparent border border-transparent text-muted w-7 h-7 p-0 rounded-md cursor-pointer inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:bg-white/[0.06] hover:text-primary"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDelete(conn);
                    }}
                    title="Delete"
                  >
                    <Trash2 size={14} />
                  </button>
                  <button
                    class="bg-accent/10 text-accent w-7 h-7 p-0 rounded-md cursor-pointer inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:bg-accent hover:text-white"
                    onclick={(e) => {
                      e.stopPropagation();
                      onConnect(conn);
                    }}
                    title="Connect"
                  >
                    <Play size={14} fill="currentColor" />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if viewMode === "grid"}
        <div class="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3">
          {#each connections as conn, index}
            <div
              class="bg-surface-raised border border-border rounded-xl relative overflow-hidden transition-all duration-150 cursor-pointer flex flex-col hover:border-border-hover hover:shadow-md group
                {selectedHostIndex === index ? 'border-accent/30' : ''}
                {justDuplicatedId === conn.id ? 'animate-flash' : ''}"
              onclick={() => onSelectHost(index)}
              ondblclick={() => onConnect(conn)}
              role="presentation"
            >
              <div
                class="absolute left-0 top-0 bottom-0 w-[3px] bg-transparent transition-colors duration-150 group-hover:bg-accent group-[.selected]:bg-accent"
              ></div>
              <div class="p-4 flex flex-col h-full box-border">
                <div class="flex justify-between items-start mb-2.5">
                  <div>
                    <h4 class="m-0 text-sm font-semibold">{conn.name}</h4>
                    <span class="text-[11px] text-muted mt-0.5 block font-mono">{conn.user}@{conn.host}:{conn.port}</span>
                  </div>
                  {#if conn.use_kerberos}
                    <span class="text-[10px] bg-accent/8 border border-accent/20 text-accent px-1.5 py-0.5 rounded-full font-semibold flex items-center gap-0.75">
                      <Shield size={14} /> KRB
                    </span>
                  {/if}
                </div>

                <div class="flex flex-wrap gap-1 mb-4">
                  {#each conn.tags as tag}
                    <span class="text-[10px] bg-white/[0.04] border border-border text-muted px-1.5 py-px rounded-full">#{tag}</span>
                  {/each}
                </div>

                <div class="flex justify-between items-center border-t border-border pt-3 mt-auto">
                  <span class="text-[11px] text-muted">
                    {conn.last_used ? `Used ${formatDate(conn.last_used, timezone)}` : "Unused"}
                  </span>

                  <div class="flex gap-0.5 items-center">
                    <button
                      class="bg-transparent border-none text-muted cursor-pointer w-7 h-7 p-0 rounded-md inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:text-primary hover:bg-white/[0.06]"
                      onclick={(e) => {
                        e.stopPropagation();
                        onCopyCommand(toSshCommand(conn), conn.id);
                      }}
                      title="Copy command"
                    >
                      {#if copiedId === conn.id}
                        <Check size={14} />
                      {:else}
                        <Copy size={14} />
                      {/if}
                    </button>
                    <button
                      class="bg-transparent border-none text-muted cursor-pointer w-7 h-7 p-0 rounded-md inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:text-primary hover:bg-white/[0.06]"
                      onclick={(e) => {
                        e.stopPropagation();
                        onEdit(conn);
                      }}
                      title="Edit"
                    >
                      <Edit2 size={14} />
                    </button>
                    <button
                      class="bg-transparent border-none text-muted cursor-pointer w-7 h-7 p-0 rounded-md inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:text-primary hover:bg-white/[0.06]"
                      onclick={(e) => {
                        e.stopPropagation();
                        onDuplicate(conn);
                      }}
                      title="Duplicate Connection"
                    >
                      <CopyPlus size={14} />
                    </button>
                    <button
                      class="bg-transparent border-none text-muted cursor-pointer w-7 h-7 p-0 rounded-md inline-flex items-center justify-center shrink-0 transition-all duration-100 hover:text-primary hover:bg-white/[0.06]"
                      onclick={(e) => {
                        e.stopPropagation();
                        onDelete(conn);
                      }}
                      title="Delete"
                    >
                      <Trash2 size={14} />
                    </button>
                    <button
                      class="bg-accent border-none text-white text-[11px] font-semibold py-1 px-2.5 rounded-md cursor-pointer flex items-center gap-1 transition-colors duration-100 hover:bg-accent-hover"
                      onclick={(e) => {
                        e.stopPropagation();
                        onConnect(conn);
                      }}
                    >
                      <Play size={14} fill="currentColor" /> Connect
                    </button>
                  </div>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      <div class="flex flex-col items-center justify-center py-16 px-8 text-center text-secondary gap-2">
        <Server size={36} class="text-muted mb-1" />
        <h3 class="m-0 text-base font-semibold text-primary">No servers configured</h3>
        <p class="m-0 text-[13px] text-muted max-w-[300px] leading-relaxed mb-2">Setup a connection profile to start managing your sessions</p>
        <button
          class="bg-accent border-none text-white py-1.5 px-3.5 rounded-lg font-semibold cursor-pointer inline-flex items-center gap-1.5 text-xs whitespace-nowrap transition-colors duration-150 hover:bg-accent-hover"
          onclick={onAddHost}
        >
          Add Host Connection
        </button>
      </div>
    {/if}
  </div>
</div>
