<script lang="ts">
  import { Play, Server, TerminalSquare, X } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { connectSSH, disconnectTab, getTerminalState } from "$lib/stores/terminal.svelte";

  interface Props {
    connections: Connection[];
    searchQuery: string;
    onSearchInput: () => void;
  }

  let { connections, searchQuery = $bindable(), onSearchInput }: Props = $props();

  const terminalState = getTerminalState();

  async function handleConnect(conn: Connection) {
    await connectSSH(conn);
  }
</script>

<div class="terminals-split-view">
  <aside class="terminals-hosts-sidebar">
    <div class="terminals-sidebar-header">
      <span class="section-title">QUICK CONNECT</span>
      <input
        type="text"
        placeholder="Search hosts..."
        bind:value={searchQuery}
        oninput={onSearchInput}
        class="cyber-input terminals-search-input"
      />
    </div>

    <div class="terminals-hosts-list">
      {#if connections.length === 0}
        <p class="terminals-empty-hint">No hosts found.</p>
      {:else}
        {#each connections as conn}
          <button class="quick-connect-btn" onclick={() => handleConnect(conn)}>
            <div class="quick-connect-info">
              <span class="quick-connect-name">{conn.name}</span>
              <span class="quick-connect-addr">{conn.user}@{conn.host}</span>
            </div>
            <span class="quick-connect-icon">
              <Play size={10} fill="currentColor" />
            </span>
          </button>
        {/each}
      {/if}
    </div>
  </aside>

  <div class="terminals-main-content">
    {#if terminalState.tabs.length > 0}
      <div class="terminal-tabs">
        {#each terminalState.tabs as tab (tab.id)}
          <div
            class="terminal-tab-btn"
            class:active={terminalState.activeTabId === tab.id}
            onclick={() => (terminalState.activeTabId = tab.id)}
            role="button"
            tabindex="0"
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") terminalState.activeTabId = tab.id;
            }}
          >
            <Server size={12} />
            <span>{tab.name}</span>
            <button
              class="tab-close-btn"
              onclick={(e) => {
                e.stopPropagation();
                disconnectTab(tab.id);
              }}
            >
              <X size={12} />
            </button>
          </div>
        {/each}
      </div>

      <div class="terminal-viewport-container">
        {#each terminalState.tabs as tab (tab.id)}
          <div
            id="terminal-{tab.id}"
            class="terminal-viewport"
            class:hidden={terminalState.activeTabId !== tab.id}
          ></div>
        {/each}
      </div>
    {:else}
      <div class="terminals-empty-state">
        <TerminalSquare size={48} class="empty-icon" />
        <h3>No active sessions</h3>
        <p>Select a host from Quick Connect to open an SSH session.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .terminals-search-input {
    width: 100%;
    box-sizing: border-box;
    font-size: 12px;
    padding: 6px 10px;
  }

  .terminals-empty-hint {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 11px;
    margin: 0;
  }

  .quick-connect-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .quick-connect-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-bottom: 2px;
  }

  .quick-connect-addr {
    font-size: 10px;
    color: var(--text-secondary);
    font-family: monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quick-connect-icon {
    color: var(--accent-cyan);
    display: flex;
    align-items: center;
    opacity: 0.7;
    flex-shrink: 0;
  }

  .terminals-empty-state :global(.empty-icon) {
    color: var(--text-muted);
    margin-bottom: 16px;
  }

  .terminals-empty-state h3 {
    font-size: 16px;
    font-weight: 600;
    margin: 0 0 6px;
    color: var(--text-primary);
  }

  .terminals-empty-state p {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0;
    max-width: 280px;
  }
</style>
