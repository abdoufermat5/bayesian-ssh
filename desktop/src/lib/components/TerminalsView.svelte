<script lang="ts">
  import {
    AppWindow,
    Layers,
    OctagonX,
    Play,
    Server,
    TerminalSquare,
    Unlink,
    X,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { tabPopOutDrag } from "$lib/actions/tabPopOutDrag";
  import {
    connectSSH,
    detachTab,
    disconnectTab,
    getTerminalState,
    popOutTab,
  } from "$lib/stores/terminal.svelte";

  interface Props {
    connections: Connection[];
    searchQuery: string;
    onSearchInput: () => void;
    onCloseAll: () => void;
    onManageSessions: () => void;
  }

  let { connections, searchQuery = $bindable(), onSearchInput, onCloseAll, onManageSessions }: Props =
    $props();

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

    {#if terminalState.externalSessionCount > 0}
      <div class="detached-compact-bar">
        <button type="button" class="detached-manage-btn" onclick={onManageSessions}>
          <Layers size={14} />
          <span>{terminalState.externalSessionCount} away</span>
        </button>
      </div>
    {/if}
  </aside>

  <div class="terminals-main-content">
    {#if terminalState.tabs.length > 0}
      <div class="terminal-tabs-bar">
        <div class="terminal-tabs">
          {#each terminalState.tabs as tab (tab.id)}
            <div
              class="terminal-tab-btn"
              class:active={terminalState.activeTabId === tab.id}
              use:tabPopOutDrag={tab.id}
              onclick={() => (terminalState.activeTabId = tab.id)}
              role="button"
              tabindex="0"
              title="Drag tab out to open in a separate window"
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") terminalState.activeTabId = tab.id;
              }}
            >
              <Server size={12} />
              <span>{tab.name}</span>
              <button
                class="tab-action-btn popout"
                title="Pop out to separate window (session keeps running)"
                onclick={(e) => {
                  e.stopPropagation();
                  void popOutTab(tab.id);
                }}
              >
                <AppWindow size={11} />
              </button>
              <button
                class="tab-action-btn detach"
                title="Run in background (hide tab, program keeps running)"
                onclick={(e) => {
                  e.stopPropagation();
                  void detachTab(tab.id);
                }}
              >
                <Unlink size={11} />
              </button>
              <button
                class="tab-close-btn"
                title="Terminate session"
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

        {#if terminalState.totalSessionCount > 0}
          <button
            type="button"
            class="close-all-sessions-btn"
            onclick={onCloseAll}
            title="Terminate all active and detached SSH sessions"
          >
            <OctagonX size={14} />
            <span>Close all</span>
          </button>
        {/if}
      </div>

      <div class="terminal-viewport-container">
        {#each terminalState.tabs as tab (tab.id)}
          <div
            class="terminal-viewport"
            class:hidden={terminalState.activeTabId !== tab.id}
          >
            <div id="terminal-{tab.id}" class="terminal-fit-target"></div>
          </div>
        {/each}
      </div>
    {:else if terminalState.externalSessionCount > 0}
      <div class="terminals-empty-state">
        <TerminalSquare size={48} class="empty-icon" />
        <h3>Sessions running elsewhere</h3>
        <p>
          {terminalState.externalSessionCount} session{terminalState.externalSessionCount === 1 ? "" : "s"} are in pop-out windows or running in the background.
          Programs keep running — reattach or dock to see live output again.
        </p>
        <button type="button" class="reattach-primary-btn" onclick={onManageSessions}>
          <Layers size={14} />
          Manage sessions
        </button>
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

  .detached-compact-bar {
    padding: 10px 12px 12px;
    border-top: 1px solid var(--border-color);
    flex-shrink: 0;
  }

  .detached-manage-btn {
    width: 100%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid rgba(0, 240, 255, 0.25);
    background: rgba(0, 240, 255, 0.06);
    color: var(--accent-cyan);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .detached-manage-btn:hover {
    background: rgba(0, 240, 255, 0.12);
  }

  .tab-action-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 1px;
    border-radius: 4px;
    display: flex;
    align-items: center;
  }

  .tab-action-btn.popout:hover {
    color: var(--text-primary);
    background: rgba(255, 255, 255, 0.08);
  }

  .tab-action-btn.detach:hover {
    color: var(--accent-cyan);
    background: rgba(0, 240, 255, 0.08);
  }

  .reattach-primary-btn {
    margin-top: 16px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 0.55rem 1rem;
    border-radius: 6px;
    border: 1px solid rgba(0, 240, 255, 0.35);
    background: rgba(0, 240, 255, 0.08);
    color: var(--accent-cyan);
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
  }

  .reattach-primary-btn:hover {
    background: rgba(0, 240, 255, 0.14);
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
    max-width: 320px;
  }
</style>
