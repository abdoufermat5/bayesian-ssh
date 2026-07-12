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

  interface Props {
    connections: Connection[];
    viewMode: "list" | "grid";
    selectedHostIndex: number;
    copiedId: string | null;
    justDuplicatedId: string | null;
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

<div class="page-view connections-view">
  <div class="page-view-header view-header">
    <div class="title-meta">
      <h2>SSH Connections</h2>
      <span class="subtitle">Bayesian ranked hosts based on connection frequency and recency</span>
    </div>
    <button class="refresh-btn" onclick={onRefresh} title="Refresh Connections">
      <RefreshCw size={14} />
    </button>
  </div>

  <div class="page-view-scroll">
  {#if connections.length > 0}
    {#if viewMode === "list"}
      <div class="list-container">
        <div class="list-header">
          <div class="col-name">Name</div>
          <div class="col-host">Address</div>
          <div class="col-tags">Tags</div>
          <div class="col-last">Last Session</div>
          <div class="col-actions"></div>
        </div>

        <div class="list-body">
          {#each connections as conn, index}
            <div
              class="list-row"
              class:selected={selectedHostIndex === index}
              class:just-duplicated={justDuplicatedId === conn.id}
              onclick={() => onSelectHost(index)}
              ondblclick={() => onConnect(conn)}
              role="row"
              tabindex="0"
              onkeydown={(e) => e.key === "Enter" && onConnect(conn)}
            >
              <div class="col-name">
                <Server size={14} class="row-icon" />
                <span class="host-name">{conn.name}</span>
                {#if conn.use_kerberos}
                  <span class="row-badge krb" title="Kerberos authentication enabled">krb5</span>
                {/if}
              </div>

              <div class="col-host font-mono">{conn.user}@{conn.host}:{conn.port}</div>

              <div class="col-tags">
                {#each conn.tags as tag}
                  <span class="tag-pill">#{tag}</span>
                {/each}
              </div>

              <div class="col-last text-muted">
                {conn.last_used ? new Date(conn.last_used).toLocaleDateString() : "Never"}
              </div>

              <div class="col-actions">
                <button
                  class="row-action-btn copy"
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
                  class="row-action-btn edit"
                  onclick={(e) => {
                    e.stopPropagation();
                    onEdit(conn);
                  }}
                  title="Edit"
                >
                  <Edit2 size={14} />
                </button>
                <button
                  class="row-action-btn duplicate"
                  onclick={(e) => {
                    e.stopPropagation();
                    onDuplicate(conn);
                  }}
                  title="Duplicate Connection"
                >
                  <CopyPlus size={14} />
                </button>
                <button
                  class="row-action-btn delete"
                  onclick={(e) => {
                    e.stopPropagation();
                    onDelete(conn);
                  }}
                  title="Delete"
                >
                  <Trash2 size={14} />
                </button>
                <button
                  class="row-action-btn connect"
                  onclick={(e) => {
                    e.stopPropagation();
                    onConnect(conn);
                  }}
                  title="Connect"
                >
                  <Play size={12} fill="currentColor" />
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if viewMode === "grid"}
      <div class="hosts-grid">
        {#each connections as conn, index}
          <div
            class="host-card"
            class:selected={selectedHostIndex === index}
            class:just-duplicated={justDuplicatedId === conn.id}
            onclick={() => onSelectHost(index)}
            ondblclick={() => onConnect(conn)}
            role="presentation"
          >
            <div class="card-left-accent"></div>
            <div class="card-content">
              <div class="card-top">
                <div class="card-title-block">
                  <h4>{conn.name}</h4>
                  <span class="host-addr font-mono">{conn.user}@{conn.host}:{conn.port}</span>
                </div>
                {#if conn.use_kerberos}
                  <span class="card-krb-badge"><Shield size={10} /> KRB</span>
                {/if}
              </div>

              <div class="card-tags-row">
                {#each conn.tags as tag}
                  <span class="tag-pill">#{tag}</span>
                {/each}
              </div>

              <div class="card-footer">
                <span class="last-used">
                  {conn.last_used ? `Used ${new Date(conn.last_used).toLocaleDateString()}` : "Unused"}
                </span>

                <div class="card-btns">
                  <button
                    class="card-btn-icon"
                    onclick={(e) => {
                      e.stopPropagation();
                      onCopyCommand(toSshCommand(conn), conn.id);
                    }}
                    title="Copy command"
                  >
                    {#if copiedId === conn.id}
                      <Check size={12} />
                    {:else}
                      <Copy size={12} />
                    {/if}
                  </button>
                  <button
                    class="card-btn-icon"
                    onclick={(e) => {
                      e.stopPropagation();
                      onEdit(conn);
                    }}
                    title="Edit"
                  >
                    <Edit2 size={12} />
                  </button>
                  <button
                    class="card-btn-icon"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDuplicate(conn);
                    }}
                    title="Duplicate Connection"
                  >
                    <CopyPlus size={12} />
                  </button>
                  <button
                    class="card-btn-icon"
                    onclick={(e) => {
                      e.stopPropagation();
                      onDelete(conn);
                    }}
                    title="Delete"
                  >
                    <Trash2 size={12} />
                  </button>
                  <button
                    class="card-connect-btn-small"
                    onclick={(e) => {
                      e.stopPropagation();
                      onConnect(conn);
                    }}
                  >
                    <Play size={10} fill="currentColor" /> Connect
                  </button>
                </div>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <div class="empty-state">
      <Server size={36} class="empty-icon" />
      <h3>No servers configured</h3>
      <p>Setup a connection profile to start managing your sessions</p>
      <button class="cyber-btn" onclick={onAddHost}>Add Host Connection</button>
    </div>
  {/if}
  </div>
</div>
