<script lang="ts">
  import { AppWindow, ExternalLink, Link2, Search, Server, X } from "lucide-svelte";
  import type { DetachedSession, PopoutSession } from "$lib/stores/terminal.svelte";

  interface Props {
    detachedSessions: DetachedSession[];
    popoutSessions: PopoutSession[];
    onClose: () => void;
    onReattach: (sessionId: string) => void | Promise<void>;
    onPopOut: (sessionId: string) => void | Promise<void>;
    onDock: (sessionId: string) => void | Promise<void>;
    onFocusPopout: (sessionId: string) => void | Promise<void>;
    onTerminateDetached: (sessionId: string) => void | Promise<void>;
    onTerminatePopout: (sessionId: string) => void | Promise<void>;
    onTerminateAll: () => void | Promise<void>;
  }

  let {
    detachedSessions,
    popoutSessions,
    onClose,
    onReattach,
    onPopOut,
    onDock,
    onFocusPopout,
    onTerminateDetached,
    onTerminatePopout,
    onTerminateAll,
  }: Props = $props();

  let query = $state("");

  const totalCount = $derived(detachedSessions.length + popoutSessions.length);

  const filteredDetached = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return detachedSessions;
    return detachedSessions.filter(
      (session) =>
        session.name.toLowerCase().includes(needle) ||
        session.connectionName.toLowerCase().includes(needle),
    );
  });

  const filteredPopouts = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return popoutSessions;
    return popoutSessions.filter(
      (session) =>
        session.name.toLowerCase().includes(needle) ||
        session.connectionName.toLowerCase().includes(needle),
    );
  });
</script>

<div class="modal-backdrop" onclick={onClose} role="dialog" aria-modal="true" aria-labelledby="session-manager-title">
  <div class="detached-manager-dialog" onclick={(e) => e.stopPropagation()}>
    <div class="detached-manager-header">
      <div>
        <h3 id="session-manager-title">Session Manager</h3>
        <p>{totalCount} session{totalCount === 1 ? "" : "s"} outside the main tab bar</p>
      </div>
      <button type="button" class="icon-btn" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="detached-manager-toolbar">
      <div class="search-box">
        <Search size={14} />
        <input type="text" placeholder="Filter sessions..." bind:value={query} class="cyber-input" />
      </div>
      {#if totalCount > 0}
        <button type="button" class="terminate-all-btn" onclick={onTerminateAll}>Terminate all</button>
      {/if}
    </div>

    <div class="detached-manager-list">
      {#if filteredPopouts.length > 0}
        <div class="session-group-label">Pop-out windows</div>
        {#each filteredPopouts as session (session.id)}
          <div class="detached-manager-item">
            <div class="detached-manager-item-main">
              <AppWindow size={14} />
              <div>
                <span class="session-name">{session.name}</span>
                <span class="session-hint">Open in separate window</span>
              </div>
            </div>
            <div class="detached-manager-item-actions">
              <button type="button" class="action-btn" title="Dock back to main window" onclick={() => onDock(session.id)}>
                <Link2 size={14} />
                <span>Dock</span>
              </button>
              <button type="button" class="action-btn" title="Focus pop-out window" onclick={() => onFocusPopout(session.id)}>
                <ExternalLink size={14} />
                <span>Focus</span>
              </button>
              <button type="button" class="action-btn danger" title="Terminate session" onclick={() => onTerminatePopout(session.id)}>
                <X size={14} />
              </button>
            </div>
          </div>
        {/each}
      {/if}

      {#if filteredDetached.length > 0}
        <div class="session-group-label">Background detached</div>
        {#each filteredDetached as session (session.id)}
          <div class="detached-manager-item">
            <div class="detached-manager-item-main">
              <Server size={14} />
              <div>
                <span class="session-name">{session.name}</span>
                <span class="session-hint">Running in background</span>
              </div>
            </div>
            <div class="detached-manager-item-actions">
              <button type="button" class="action-btn" title="Reattach here" onclick={() => onReattach(session.id)}>
                <Link2 size={14} />
                <span>Reattach</span>
              </button>
              <button type="button" class="action-btn" title="Open in new window" onclick={() => onPopOut(session.id)}>
                <AppWindow size={14} />
                <span>Pop out</span>
              </button>
              <button type="button" class="action-btn danger" title="Terminate session" onclick={() => onTerminateDetached(session.id)}>
                <X size={14} />
              </button>
            </div>
          </div>
        {/each}
      {/if}

      {#if filteredPopouts.length === 0 && filteredDetached.length === 0}
        <div class="detached-manager-empty">
          {query.trim() ? "No sessions match your filter." : "No detached or pop-out sessions."}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .detached-manager-dialog {
    width: min(720px, calc(100vw - 32px));
    max-height: min(80vh, 720px);
    display: flex;
    flex-direction: column;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 12px;
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.45);
    overflow: hidden;
  }

  .detached-manager-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 18px 20px 12px;
    border-bottom: 1px solid var(--border-color);
  }

  .detached-manager-header h3 {
    margin: 0 0 4px;
    font-size: 16px;
    color: var(--text-primary);
  }

  .detached-manager-header p {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .detached-manager-toolbar {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border-color);
  }

  .search-box {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-muted);
  }

  .search-box input {
    border: none;
    background: transparent;
    padding-left: 0;
  }

  .terminate-all-btn {
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid rgba(239, 68, 68, 0.35);
    background: rgba(239, 68, 68, 0.08);
    color: #fca5a5;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }

  .detached-manager-list {
    overflow-y: auto;
    padding: 12px 20px 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .session-group-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-top: 4px;
    margin-bottom: 2px;
  }

  .detached-manager-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.02);
  }

  .detached-manager-item-main {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    color: var(--accent-cyan);
  }

  .session-name {
    display: block;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .session-hint {
    display: block;
    font-size: 11px;
    color: var(--text-muted);
  }

  .detached-manager-item-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .action-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-radius: 8px;
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-secondary);
    font-size: 11px;
    cursor: pointer;
  }

  .action-btn:hover {
    color: var(--text-primary);
    background: rgba(255, 255, 255, 0.04);
  }

  .action-btn.danger:hover {
    color: #fca5a5;
    border-color: rgba(239, 68, 68, 0.35);
    background: rgba(239, 68, 68, 0.08);
  }

  .detached-manager-empty {
    padding: 36px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
