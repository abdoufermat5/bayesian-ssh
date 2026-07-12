<script lang="ts">
  import {
    TerminalSquare,
    Server,
    Clock,
    Settings,
    FolderPlus,
    ChevronLeft,
    ChevronRight,
    Layers,
    KeyRound,
    ShieldCheck,
    AppWindow,
  } from "lucide-svelte";
  import type { AppTab, ConnectionStats, EnvInfo } from "$lib/types";

  interface Props {
    activeTab: AppTab;
    onTabChange: (tab: AppTab) => void;
    environments: EnvInfo[];
    activeEnv: string;
    onSwitchEnv: (name: string) => void;
    onShowEnvModal: () => void;
    stats: ConnectionStats | null;
    sidebarCollapsed: boolean;
    onToggleSidebar: () => void;
    terminalCount: number;
    tabCount: number;
    externalSessionCount: number;
    allTags: string[];
    selectedTag: string | null;
    onTagSelect: (tag: string | null) => void;
    agentActive: boolean;
    agentKeys: string[];
    onStartAgent: () => void;
    onShowAgentModal: () => void;
    kerberosHealth: "missing" | "expired" | "warning" | "valid" | "unavailable";
    kerberosRemainingLabel: string;
    kerberosPrincipal: string | null;
    onShowKerberosModal: () => void;
    onShowSessionManager: () => void;
    onGoToTerminals: () => void;
    onSearchMostUsed: (name: string) => void;
  }

  let {
    activeTab,
    onTabChange,
    environments,
    activeEnv,
    onSwitchEnv,
    onShowEnvModal,
    stats,
    sidebarCollapsed,
    onToggleSidebar,
    terminalCount,
    tabCount,
    externalSessionCount,
    allTags,
    selectedTag,
    onTagSelect,
    agentActive,
    agentKeys,
    onStartAgent,
    onShowAgentModal,
    kerberosHealth,
    kerberosRemainingLabel,
    kerberosPrincipal,
    onShowKerberosModal,
    onShowSessionManager,
    onGoToTerminals,
    onSearchMostUsed,
  }: Props = $props();

  function handleSessionsCardClick() {
    onGoToTerminals();
    if (externalSessionCount > 0) {
      onShowSessionManager();
    }
  }
</script>

<aside class="sidebar">
  <div class="sidebar-header">
    <div class="logo-container">
      <TerminalSquare class="logo-icon" size={24} />
      <span class="logo-text">BSSH</span>
    </div>

    <div class="env-widget">
      <div class="env-header">
        <span class="section-title">PROFILE</span>
        <button class="small-icon-btn" onclick={onShowEnvModal} title="Manage Profiles">
          <FolderPlus size={14} />
        </button>
      </div>
      <div class="select-wrapper">
        <select
          class="cyber-select"
          value={activeEnv}
          onchange={(e) => onSwitchEnv((e.target as HTMLSelectElement).value)}
        >
          {#each environments as env}
            <option value={env.name}>{env.name}</option>
          {/each}
        </select>
      </div>
    </div>

    <nav class="nav-menu">
      <button
        class="nav-item"
        class:active={activeTab === "connections"}
        onclick={() => onTabChange("connections")}
        title="Hosts"
      >
        <Server size={18} />
        <span class="nav-label">Hosts</span>
      </button>
      <button
        class="nav-item"
        class:active={activeTab === "terminals"}
        onclick={() => onTabChange("terminals")}
        title="Terminals"
      >
        <TerminalSquare size={18} />
        <span class="nav-label">Terminals</span>
        {#if terminalCount > 0}
          <span class="badge">{terminalCount}</span>
        {/if}
      </button>
      <button
        class="nav-item"
        class:active={activeTab === "history"}
        onclick={() => onTabChange("history")}
        title="Session logs"
      >
        <Clock size={18} />
        <span class="nav-label">Logs</span>
      </button>
      <button
        class="nav-item"
        class:active={activeTab === "settings"}
        onclick={() => onTabChange("settings")}
        title="Settings"
      >
        <Settings size={18} />
        <span class="nav-label">Settings</span>
      </button>

      {#if sidebarCollapsed && externalSessionCount > 0}
        <button
          class="nav-item session-alert-nav"
          onclick={onShowSessionManager}
          title="{externalSessionCount} session(s) running elsewhere"
        >
          <Layers size={18} />
          <span class="badge alert">{externalSessionCount}</span>
        </button>
      {/if}
    </nav>
  </div>

  <div class="sidebar-scroll">
    {#if !sidebarCollapsed}
      <div class="runtime-panel">
        <span class="section-title">RUNTIME</span>

        <button
          type="button"
          class="runtime-card sessions-card"
          class:has-away={externalSessionCount > 0}
          onclick={handleSessionsCardClick}
        >
          <div class="runtime-card-top">
            <TerminalSquare size={16} />
            <span>Sessions</span>
            {#if externalSessionCount > 0}
              <span class="runtime-pill alert">{externalSessionCount} away</span>
            {/if}
          </div>
          <div class="runtime-card-stats">
            <span><strong>{tabCount}</strong> tabs</span>
            <span><strong>{terminalCount}</strong> total</span>
          </div>
          <span class="runtime-card-hint">
            {#if externalSessionCount > 0}
              Click to manage background & pop-out sessions
            {:else if tabCount > 0}
              Click to jump to terminal tabs
            {:else}
              Open a host to start a session
            {/if}
          </span>
        </button>

        <div class="runtime-actions">
          <button
            type="button"
            class="runtime-action-btn"
            class:active={agentActive}
            onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
            title={agentActive ? "SSH Agent active" : "Start SSH Agent"}
          >
            <KeyRound size={14} />
            <span class="runtime-action-label">Agent</span>
            <span class="runtime-action-meta">
              {#if agentActive}
                {agentKeys.length} keys
              {:else}
                Off
              {/if}
            </span>
          </button>

          <button
            type="button"
            class="runtime-action-btn kerberos {kerberosHealth}"
            onclick={onShowKerberosModal}
            title="Kerberos ticket"
            disabled={kerberosHealth === "unavailable"}
          >
            <ShieldCheck size={14} />
            <span class="runtime-action-label">Kerberos</span>
            <span class="runtime-action-meta">
              {#if kerberosHealth === "unavailable"}
                N/A
              {:else if kerberosHealth === "valid" || kerberosHealth === "warning"}
                {kerberosRemainingLabel}
              {:else if kerberosHealth === "missing"}
                Missing
              {:else}
                Expired
              {/if}
            </span>
          </button>
        </div>

        {#if externalSessionCount > 0}
          <button type="button" class="runtime-manage-btn" onclick={onShowSessionManager}>
            <Layers size={14} />
            <span>Manage running sessions</span>
            <AppWindow size={13} />
          </button>
        {/if}

        {#if kerberosPrincipal && kerberosHealth !== "unavailable"}
          <div class="runtime-footnote" title={kerberosPrincipal}>
            {kerberosPrincipal}
          </div>
        {/if}
      </div>
    {/if}

    {#if stats && !sidebarCollapsed}
      <div class="sidebar-stats compact-metrics">
        <span class="section-title">METRICS</span>
        <div class="stat-row">
          <span>Hosts</span>
          <span>{stats.total_connections}</span>
        </div>
        {#if stats.most_used}
          <button type="button" class="stat-row stat-link" onclick={() => onSearchMostUsed(stats?.most_used?.name || "")}>
            <span>Top host</span>
            <span class="text-glow">{stats.most_used.name}</span>
          </button>
        {/if}
      </div>
    {/if}

    {#if allTags.length > 0 && !sidebarCollapsed}
      <div class="quick-tags">
        <span class="section-title">TAGS</span>
        <div class="tags-list">
          <button class="tag-btn" class:active={selectedTag === null} onclick={() => onTagSelect(null)}>
            All
          </button>
          {#each allTags as tag}
            <button class="tag-btn" class:active={selectedTag === tag} onclick={() => onTagSelect(tag)}>
              #{tag}
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <button
    class="sidebar-toggle-btn"
    onclick={onToggleSidebar}
    title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
    aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
  >
    {#if sidebarCollapsed}
      <ChevronRight size={16} />
    {:else}
      <ChevronLeft size={16} />
    {/if}
  </button>
</aside>

<style>
  .runtime-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 1.25rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid var(--border-color);
  }

  .runtime-card {
    width: 100%;
    text-align: left;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.02);
    padding: 10px 12px;
    cursor: pointer;
    color: inherit;
    transition: border-color 0.2s, background 0.2s;
  }

  .runtime-card:hover {
    border-color: rgba(0, 240, 255, 0.3);
    background: rgba(0, 240, 255, 0.04);
  }

  .runtime-card.has-away {
    border-color: rgba(0, 240, 255, 0.25);
    background: rgba(0, 240, 255, 0.05);
  }

  .runtime-card-top {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 6px;
  }

  .runtime-pill {
    margin-left: auto;
    font-size: 10px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.06);
    color: var(--text-secondary);
  }

  .runtime-pill.alert {
    background: rgba(0, 240, 255, 0.12);
    color: var(--accent-cyan);
  }

  .runtime-card-stats {
    display: flex;
    gap: 12px;
    font-size: 11px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .runtime-card-stats strong {
    color: var(--accent-cyan);
    font-weight: 700;
  }

  .runtime-card-hint {
    font-size: 10px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .runtime-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .runtime-action-btn {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--border-color);
    background: rgba(255, 255, 255, 0.02);
    cursor: pointer;
    color: var(--text-secondary);
    min-width: 0;
  }

  .runtime-action-btn:hover:not(:disabled) {
    border-color: rgba(255, 255, 255, 0.12);
    background: rgba(255, 255, 255, 0.04);
    color: var(--text-primary);
  }

  .runtime-action-btn.active {
    border-color: rgba(0, 240, 255, 0.25);
    color: var(--accent-cyan);
  }

  .runtime-action-btn.kerberos.valid {
    border-color: rgba(52, 211, 153, 0.25);
    color: #34d399;
  }

  .runtime-action-btn.kerberos.warning {
    border-color: rgba(251, 191, 36, 0.35);
    color: #fbbf24;
  }

  .runtime-action-btn.kerberos.expired,
  .runtime-action-btn.kerberos.missing {
    border-color: rgba(248, 113, 113, 0.35);
    color: #f87171;
  }

  .runtime-action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .runtime-action-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .runtime-action-meta {
    font-size: 11px;
    font-weight: 600;
    font-family: monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .runtime-manage-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px dashed rgba(0, 240, 255, 0.3);
    background: rgba(0, 240, 255, 0.06);
    color: var(--accent-cyan);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }

  .runtime-manage-btn:hover {
    background: rgba(0, 240, 255, 0.1);
  }

  .runtime-footnote {
    font-size: 10px;
    font-family: monospace;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0 2px;
  }

  .compact-metrics {
    margin-bottom: 1rem;
  }

  .stat-link {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    font: inherit;
    color: inherit;
    text-align: inherit;
  }

  .stat-link:hover .text-glow {
    color: var(--accent-cyan);
  }

  .session-alert-nav {
    margin-top: 4px;
    border: 1px solid rgba(0, 240, 255, 0.25);
  }

  .badge.alert {
    background: rgba(0, 240, 255, 0.15);
    color: var(--accent-cyan);
  }
</style>
