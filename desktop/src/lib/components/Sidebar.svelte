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
    Tag,
  } from "lucide-svelte";
  import type { AppTab, ConnectionStats, EnvInfo } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";

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

  function handleSessionsClick() {
    onGoToTerminals();
    if (externalSessionCount > 0) {
      onShowSessionManager();
    }
  }
</script>

<aside
  class="flex flex-col border-r border-border bg-surface shrink-0 relative z-20 overflow-visible min-h-0 transition-all duration-300 ease-out"
  style="width: {sidebarCollapsed ? 'var(--sidebar-w-collapsed)' : 'var(--sidebar-w)'}; padding: {sidebarCollapsed ? '12px 6px' : '14px 10px'};"
>
  <!-- Header -->
  <div class="shrink-0">
    <!-- Profile Selector -->
    {#if !sidebarCollapsed}
      <div class="mb-4">
        <div class="flex justify-between items-center mb-1">
          <span class="text-[10px] font-bold uppercase tracking-wider text-muted px-1">Profile</span>
          <button
            class="bg-transparent border-none text-muted cursor-pointer p-0.5 rounded flex items-center transition-colors hover:text-primary hover:bg-white/5"
            onclick={onShowEnvModal}
            title="Manage Profiles"
          >
            <FolderPlus size={13} />
          </button>
        </div>
          <CustomSelect
            options={environments.map((env) => ({ value: env.name, label: env.name }))}
            value={activeEnv}
            onChange={(val) => onSwitchEnv(val)}
          />
        </div>
      {/if}

    <!-- Main Navigation -->
    <nav class="flex flex-col gap-0.5 mb-4 shrink-0">
      {#each [
        { tab: "connections" as AppTab, icon: Server, label: "Hosts" },
        { tab: "terminals" as AppTab, icon: TerminalSquare, label: "Terminals", badge: terminalCount },
        { tab: "keys" as AppTab, icon: KeyRound, label: "Keys" },
        { tab: "audit" as AppTab, icon: ShieldCheck, label: "Audit" },
        { tab: "history" as AppTab, icon: Clock, label: "Logs" },
        { tab: "settings" as AppTab, icon: Settings, label: "Settings" },
      ] as item}
        <button
          class="flex items-center gap-2.5 w-full bg-transparent border-none text-muted py-1.5 px-2.5 rounded-md cursor-pointer text-xs font-medium text-left transition-all duration-100 relative
            {sidebarCollapsed ? 'justify-center' : ''}
            {activeTab === item.tab ? 'text-primary bg-accent/10 border-l-2 border-accent font-semibold' : 'hover:text-primary hover:bg-white/[0.04]'}"
          onclick={() => onTabChange(item.tab)}
          title={item.label}
        >
          <item.icon size={16} class={activeTab === item.tab ? "text-accent" : ""} />
          {#if !sidebarCollapsed}
            <span>{item.label}</span>
            {#if item.badge && item.badge > 0}
              <span class="ml-auto min-w-[16px] h-[16px] px-1 rounded-full bg-accent/20 text-accent text-[10px] font-bold inline-flex items-center justify-center">
                {item.badge}
              </span>
            {/if}
          {/if}
        </button>
      {/each}
    </nav>
  </div>

  <!-- Scrollable Runtime & Filters -->
  <div class="flex-1 min-h-0 overflow-y-auto flex flex-col gap-4 scrollbar-none">
    {#if !sidebarCollapsed}
      <!-- Quick Status Indicators -->
      <div class="flex flex-col gap-1.5 pt-2 border-t border-border">
        <span class="text-[10px] font-bold uppercase tracking-wider text-muted px-1 mb-0.5">Status</span>

        <!-- Agent Status -->
        <button
          type="button"
          class="flex items-center justify-between py-1.5 px-2 rounded-md border text-xs cursor-pointer transition-all border-border bg-surface-input/60 hover:bg-surface-input"
          onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
        >
          <span class="flex items-center gap-1.5 text-muted">
            <KeyRound size={13} class={agentActive ? "text-emerald-400" : ""} />
            Agent
          </span>
          <span class="font-mono text-[11px] font-semibold {agentActive ? 'text-emerald-400' : 'text-muted'}">
            {agentActive ? `${agentKeys.length} keys` : 'Off'}
          </span>
        </button>

        <!-- Kerberos Status -->
        {#if kerberosHealth !== "unavailable"}
          <button
            type="button"
            class="flex items-center justify-between py-1.5 px-2 rounded-md border text-xs cursor-pointer transition-all border-border bg-surface-input/60 hover:bg-surface-input"
            onclick={onShowKerberosModal}
          >
            <span class="flex items-center gap-1.5 text-muted">
              <ShieldCheck size={13} class={kerberosHealth === 'valid' ? 'text-emerald-400' : 'text-amber-400'} />
              Kerberos
            </span>
            <span class="font-mono text-[11px] font-semibold {kerberosHealth === 'valid' ? 'text-emerald-400' : 'text-amber-400'}">
              {kerberosHealth === 'valid' ? kerberosRemainingLabel : 'Ticket'}
            </span>
          </button>
        {/if}

        <!-- Detached Sessions Badge -->
        {#if externalSessionCount > 0}
          <button
            type="button"
            class="flex items-center justify-between py-1.5 px-2 rounded-md border border-accent/30 bg-accent/10 text-accent text-xs font-semibold cursor-pointer hover:bg-accent/15"
            onclick={onShowSessionManager}
          >
            <span class="flex items-center gap-1.5">
              <Layers size={13} />
              Background
            </span>
            <span class="text-[10px] font-bold">{externalSessionCount} away</span>
          </button>
        {/if}
      </div>

      <!-- Tag Filters -->
      {#if allTags.length > 0}
        <div class="pt-2 border-t border-border">
          <span class="text-[10px] font-bold uppercase tracking-wider text-muted px-1 flex items-center gap-1 mb-1.5">
            <Tag size={11} />
            Tags
          </span>
          <div class="flex flex-wrap gap-1">
            <button
              class="text-[11px] py-0.5 px-2 rounded cursor-pointer transition-all border
                {selectedTag === null
                  ? 'border-accent bg-accent/15 text-accent font-semibold'
                  : 'border-border bg-surface-input text-muted hover:text-primary hover:border-border-hover'}"
              onclick={() => onTagSelect(null)}
            >
              All
            </button>
            {#each allTags as tag}
              <button
                class="text-[11px] py-0.5 px-2 rounded cursor-pointer transition-all border
                  {selectedTag === tag
                    ? 'border-accent bg-accent/15 text-accent font-semibold'
                    : 'border-border bg-surface-input text-muted hover:text-primary hover:border-border-hover'}"
                onclick={() => onTagSelect(tag)}
              >
                #{tag}
              </button>
            {/each}
          </div>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Sidebar Toggle -->
  <button
    class="absolute top-1/2 -right-3 -translate-y-1/2 bg-surface-raised border border-border-hover text-secondary cursor-pointer w-[24px] h-[24px] rounded-full flex items-center justify-center z-50 shadow-md transition-all hover:text-accent hover:border-accent"
    onclick={onToggleSidebar}
    title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
  >
    {#if sidebarCollapsed}
      <ChevronRight size={13} />
    {:else}
      <ChevronLeft size={13} />
    {/if}
  </button>
</aside>
