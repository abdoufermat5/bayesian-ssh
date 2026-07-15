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
    kerberosDefaultRealm: string | null;
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
    kerberosDefaultRealm,
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

<aside
  class="flex flex-col border-r border-border bg-surface shrink-0 relative z-20 overflow-visible min-h-0 transition-all duration-300 ease-out"
  style="width: {sidebarCollapsed ? 'var(--sidebar-w-collapsed)' : 'var(--sidebar-w)'}; padding: {sidebarCollapsed ? '16px 8px' : '16px 12px'};"
>
  <!-- Header -->
  <div class="shrink-0">
    <!-- Logo -->
    <div class="flex items-center gap-2.5 mb-6 {sidebarCollapsed ? 'justify-center' : 'pl-1'}">
      <TerminalSquare class="text-accent shrink-0" size={22} />
      {#if !sidebarCollapsed}
        <span class="text-sm font-bold tracking-wide text-primary">BSSH</span>
      {/if}
    </div>

    <!-- Profile Selector -->
    {#if !sidebarCollapsed}
      <div class="mb-5">
        <div class="flex justify-between items-center">
          <span class="text-[10px] font-semibold uppercase tracking-wider text-muted pl-1">Profile</span>
          <button
            class="bg-transparent border-none text-muted cursor-pointer p-1 rounded-md flex items-center transition-colors duration-100 hover:text-primary hover:bg-white/5"
            onclick={onShowEnvModal}
            title="Manage Profiles"
          >
            <FolderPlus size={14} />
          </button>
        </div>
        <div class="mt-1">
          <select
            class="w-full bg-surface-input border border-border text-primary py-1.5 px-2.5 rounded-lg outline-none text-xs cursor-pointer appearance-none transition-colors duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.15)]"
            value={activeEnv}
            onchange={(e) => onSwitchEnv((e.target as HTMLSelectElement).value)}
          >
            {#each environments as env}
              <option value={env.name}>{env.name}</option>
            {/each}
          </select>
        </div>
      </div>
    {/if}

    <!-- Navigation -->
    <nav class="flex flex-col gap-0.5 mb-5 shrink-0">
      {#each [
        { tab: "connections" as AppTab, icon: Server, label: "Hosts" },
        { tab: "terminals" as AppTab, icon: TerminalSquare, label: "Terminals" },
        { tab: "history" as AppTab, icon: Clock, label: "Logs" },
        { tab: "settings" as AppTab, icon: Settings, label: "Settings" },
      ] as item}
        <button
          class="flex items-center gap-2.5 w-full bg-transparent border-none text-muted py-2 px-2.5 rounded-lg cursor-pointer text-[13px] font-medium text-left transition-all duration-100 relative
            {sidebarCollapsed ? 'justify-center' : ''}
            {activeTab === item.tab ? 'text-primary bg-accent-muted' : 'hover:text-primary hover:bg-white/5'}"
          onclick={() => onTabChange(item.tab)}
          title={item.label}
        >
          {#if activeTab === item.tab}
            <span class="absolute left-0 top-[20%] h-[60%] w-[3px] bg-accent rounded-r-sm"></span>
          {/if}
          <item.icon size={18} />
          {#if !sidebarCollapsed}
            <span>{item.label}</span>
            {#if item.tab === "terminals" && terminalCount > 0}
              <span class="ml-auto min-w-[18px] h-[18px] px-1 rounded-full bg-accent-muted text-accent text-[10px] font-bold inline-flex items-center justify-center">
                {terminalCount}
              </span>
            {/if}
          {/if}
        </button>
      {/each}

      {#if sidebarCollapsed && externalSessionCount > 0}
        <button
          class="flex items-center justify-center gap-2 w-full bg-transparent border border-accent/25 text-muted py-2 rounded-lg cursor-pointer mt-1 transition-colors duration-100 hover:text-primary hover:bg-white/5"
          onclick={onShowSessionManager}
          title="{externalSessionCount} session(s) running elsewhere"
        >
          <Layers size={18} />
          <span class="min-w-[18px] h-[18px] px-1 rounded-full bg-accent-muted text-accent text-[10px] font-bold inline-flex items-center justify-center">
            {externalSessionCount}
          </span>
        </button>
      {/if}
    </nav>
  </div>

  <!-- Scrollable Content -->
  <div class="flex-1 min-h-0 overflow-y-auto overflow-x-hidden flex flex-col overscroll-contain">
    {#if !sidebarCollapsed}
      <!-- Runtime Panel -->
      <div class="flex flex-col gap-2 mb-4 pb-4 border-b border-border">
        <span class="text-[10px] font-semibold uppercase tracking-wider text-muted pl-1">Runtime</span>

        <!-- Sessions Card -->
        <button
          type="button"
          class="w-full text-left border rounded-xl p-2.5 cursor-pointer transition-all duration-150
            {externalSessionCount > 0
              ? 'border-accent/25 bg-accent/5 hover:border-accent/35 hover:bg-accent/8'
              : 'border-border bg-white/[0.02] hover:border-accent/25 hover:bg-accent/[0.04]'}"
          onclick={handleSessionsCardClick}
        >
          <div class="flex items-center gap-2 text-xs font-semibold text-primary mb-1.5">
            <TerminalSquare size={15} />
            <span>Sessions</span>
            {#if externalSessionCount > 0}
              <span class="ml-auto text-[10px] font-bold px-1.5 py-0.5 rounded-full bg-accent-muted text-accent">
                {externalSessionCount} away
              </span>
            {/if}
          </div>
          <div class="flex gap-3 text-[11px] text-muted mb-1">
            <span><strong class="text-accent font-bold">{tabCount}</strong> tabs</span>
            <span><strong class="text-accent font-bold">{terminalCount}</strong> total</span>
          </div>
          <span class="text-[10px] text-muted leading-snug">
            {#if externalSessionCount > 0}
              Click to manage background & pop-out sessions
            {:else if tabCount > 0}
              Click to jump to terminal tabs
            {:else}
              Open a host to start a session
            {/if}
          </span>
        </button>

        <!-- Agent & Kerberos -->
        <div class="grid grid-cols-2 gap-1.5">
          <button
            type="button"
            class="flex flex-col items-start gap-0.5 p-2 rounded-lg border cursor-pointer min-w-0 transition-all duration-100
              {agentActive
                ? 'border-accent/25 text-accent hover:bg-accent/5'
                : 'border-border bg-white/[0.02] text-secondary hover:border-border-hover hover:bg-white/[0.04] hover:text-primary'}"
            onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
            title={agentActive ? "SSH Agent active" : "Start SSH Agent"}
          >
            <KeyRound size={14} />
            <span class="text-[10px] font-bold tracking-wider uppercase">Agent</span>
            <span class="text-[11px] font-semibold font-mono overflow-hidden text-ellipsis whitespace-nowrap max-w-full">
              {#if agentActive}
                {agentKeys.length} keys
              {:else}
                Off
              {/if}
            </span>
          </button>

          <button
            type="button"
            class="flex flex-col items-start gap-0.5 p-2 rounded-lg border cursor-pointer min-w-0 transition-all duration-100
              {kerberosHealth === 'valid' ? 'border-success/25 text-success' :
               kerberosHealth === 'warning' ? 'border-warning/35 text-warning' :
               kerberosHealth === 'expired' || kerberosHealth === 'missing' ? 'border-danger/35 text-danger' :
               'border-border bg-white/[0.02] text-secondary'}"
            class:opacity-50={kerberosHealth === "unavailable"}
            class:cursor-not-allowed={kerberosHealth === "unavailable"}
            onclick={onShowKerberosModal}
            title="Kerberos ticket"
            disabled={kerberosHealth === "unavailable"}
          >
            <ShieldCheck size={14} />
            <span class="text-[10px] font-bold tracking-wider uppercase">Kerberos</span>
            <span class="text-[11px] font-semibold font-mono overflow-hidden text-ellipsis whitespace-nowrap max-w-full">
              {#if kerberosHealth === "unavailable"}
                N/A
              {:else if kerberosHealth === "valid" || kerberosHealth === "warning"}
                {kerberosRemainingLabel}
              {:else if kerberosHealth === "missing"}
                {kerberosDefaultRealm ?? "No ticket"}
              {:else}
                Expired
              {/if}
            </span>
          </button>
        </div>

        {#if externalSessionCount > 0}
          <button
            type="button"
            class="flex items-center justify-center gap-2 w-full py-2 px-2.5 rounded-lg border border-dashed border-accent/30 bg-accent/5 text-accent text-[11px] font-semibold cursor-pointer transition-colors duration-100 hover:bg-accent/10"
            onclick={onShowSessionManager}
          >
            <Layers size={14} />
            <span>Manage running sessions</span>
            <AppWindow size={13} />
          </button>
        {/if}

        {#if kerberosPrincipal && kerberosHealth !== "unavailable"}
          <div class="text-[10px] font-mono text-muted overflow-hidden text-ellipsis whitespace-nowrap px-0.5" title={kerberosPrincipal}>
            {kerberosPrincipal}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Metrics -->
    {#if stats && !sidebarCollapsed}
      <div class="bg-white/[0.02] border border-border rounded-lg p-3 mb-4">
        <span class="text-[10px] font-semibold uppercase tracking-wider text-muted block mb-2 pl-0.5">Metrics</span>
        <div class="flex justify-between text-xs text-secondary mb-1">
          <span>Hosts</span>
          <span>{stats.total_connections}</span>
        </div>
        {#if stats.most_used}
          <button
            type="button"
            class="flex justify-between w-full bg-transparent border-none cursor-pointer p-0 font-inherit text-inherit text-left text-xs text-secondary hover:[&_.top-host]:text-accent"
            onclick={() => onSearchMostUsed(stats?.most_used?.name || "")}
          >
            <span>Top host</span>
            <span class="top-host text-accent font-semibold">{stats.most_used.name}</span>
          </button>
        {/if}
      </div>
    {/if}

    <!-- Tags -->
    {#if allTags.length > 0 && !sidebarCollapsed}
      <div>
        <span class="text-[10px] font-semibold uppercase tracking-wider text-muted block mb-2 pl-1">Tags</span>
        <div class="flex flex-wrap gap-1">
          <button
            class="border text-[11px] py-0.5 px-2 rounded-full cursor-pointer transition-all duration-100
              {selectedTag === null
                ? 'border-accent/30 bg-accent/8 text-accent'
                : 'border-border bg-white/[0.03] text-muted hover:text-accent hover:border-accent/30 hover:bg-accent/5'}"
            onclick={() => onTagSelect(null)}
          >
            All
          </button>
          {#each allTags as tag}
            <button
              class="border text-[11px] py-0.5 px-2 rounded-full cursor-pointer transition-all duration-100
                {selectedTag === tag
                  ? 'border-accent/30 bg-accent/8 text-accent'
                  : 'border-border bg-white/[0.03] text-muted hover:text-accent hover:border-accent/30 hover:bg-accent/5'}"
              onclick={() => onTagSelect(tag)}
            >
              {tag}
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <!-- Sidebar Toggle -->
  <button
    class="absolute top-1/2 -right-3 -translate-y-1/2 bg-surface-raised border border-border-hover text-secondary cursor-pointer w-[26px] h-[26px] rounded-full flex items-center justify-center z-50 shadow-md transition-all duration-100 hover:text-accent hover:border-accent/40 hover:scale-110"
    onclick={onToggleSidebar}
    title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
    aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
  >
    {#if sidebarCollapsed}
      <ChevronRight size={14} />
    {:else}
      <ChevronLeft size={14} />
    {/if}
  </button>
</aside>
