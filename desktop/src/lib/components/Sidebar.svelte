<script lang="ts">
  import {
    TerminalSquare,
    Server,
    Clock,
    Settings,
    FolderPlus,
    ChevronLeft,
    ChevronRight,
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
    allTags: string[];
    selectedTag: string | null;
    onTagSelect: (tag: string | null) => void;
    agentActive: boolean;
    agentKeys: string[];
    onStartAgent: () => void;
    onShowAgentModal: () => void;
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
    allTags,
    selectedTag,
    onTagSelect,
    agentActive,
    agentKeys,
    onStartAgent,
    onShowAgentModal,
    onSearchMostUsed,
  }: Props = $props();
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
      >
        <Server size={18} />
        <span class="nav-label">Hosts</span>
      </button>
      <button
        class="nav-item"
        class:active={activeTab === "terminals"}
        onclick={() => onTabChange("terminals")}
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
      >
        <Clock size={18} />
        <span class="nav-label">Logs</span>
      </button>
      <button
        class="nav-item"
        class:active={activeTab === "settings"}
        onclick={() => onTabChange("settings")}
      >
        <Settings size={18} />
        <span class="nav-label">Settings</span>
      </button>
    </nav>
  </div>

  <div class="sidebar-scroll">
  {#if stats && !sidebarCollapsed}
    <div class="sidebar-stats">
      <span class="section-title">METRICS</span>
      <div class="stat-row">
        <span>Registered:</span>
        <span>{stats.total_connections} hosts</span>
      </div>
      {#if stats.most_used}
        <div class="stat-row" onclick={() => onSearchMostUsed(stats?.most_used?.name || "")}>
          <span>Frequent:</span>
          <span class="text-glow">{stats.most_used.name}</span>
        </div>
      {/if}
    </div>
  {/if}

  {#if !sidebarCollapsed}
    <div class="sidebar-stats">
      <span class="section-title">SSH AGENT</span>
      <div class="stat-row">
        <span>Status:</span>
        {#if agentActive}
          <span
            class="status-indicator active"
            onclick={onShowAgentModal}
            style="cursor: pointer; display: flex; align-items: center; gap: 6px;"
          >
            <span
              class="dot"
              style="width: 8px; height: 8px; border-radius: 50%; background-color: var(--accent-cyan); display: inline-block; box-shadow: 0 0 8px var(--accent-cyan);"
            ></span> Active
          </span>
        {:else}
          <button
            class="agent-start-btn"
            onclick={onStartAgent}
            style="background: none; border: none; color: var(--text-muted); cursor: pointer; display: flex; align-items: center; gap: 6px; padding: 0; font-size: 11px;"
          >
            <span
              class="dot"
              style="width: 8px; height: 8px; border-radius: 50%; background-color: var(--text-muted); display: inline-block;"
            ></span> Start Agent
          </button>
        {/if}
      </div>
      {#if agentActive}
        <div class="stat-row clickable" onclick={onShowAgentModal} style="cursor: pointer;">
          <span>Loaded Keys:</span>
          <span class="text-glow" style="color: var(--accent-cyan);">{agentKeys.length} keys</span>
        </div>
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
