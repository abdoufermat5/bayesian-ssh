<script lang="ts">
  import {
    ChevronLeft,
    ChevronRight,
    Clock,
    Code,
    FolderPlus,
    HardDrive,
    KeyRound,
    Layers,
    Network,
    Server,
    Settings,
    ShieldCheck,
    Tag,
    TerminalSquare,
  } from "lucide-svelte";
  import type { AppTab, ConnectionStats, DesktopSettings, EnvInfo } from "$lib/types";
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
    onShowSnippetsModal?: () => void;
    settings?: DesktopSettings;
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
    onShowSnippetsModal,
    settings,
  }: Props = $props();

  const navItems = $derived([
    { tab: "connections" as AppTab, icon: Server, label: "Hosts" },
    { tab: "terminals" as AppTab, icon: TerminalSquare, label: "Terminals", badge: terminalCount },
    ...(settings?.enable_sftp !== false ? [{ tab: "sftp" as AppTab, icon: HardDrive, label: "SFTP Files" }] : []),
    ...(settings?.enable_tunneling !== false ? [{ tab: "tunnels" as AppTab, icon: Network, label: "Tunnels" }] : []),
    { tab: "keys" as AppTab, icon: KeyRound, label: "Keys" },
    { tab: "audit" as AppTab, icon: ShieldCheck, label: "Audit" },
    { tab: "history" as AppTab, icon: Clock, label: "Logs" },
    { tab: "settings" as AppTab, icon: Settings, label: "Settings" },
  ]);
</script>

<aside
  class="flex flex-col border-r border-border bg-surface shrink-0 relative z-20 overflow-visible min-h-0 transition-all duration-200 select-none"
  style="width: {sidebarCollapsed ? 'var(--sidebar-w-collapsed)' : 'var(--sidebar-w)'};"
>
  <!-- Top Profile Header -->
  <div class="p-3 border-b border-border/80 shrink-0">
    {#if !sidebarCollapsed}
      <div class="flex items-center justify-between gap-1 mb-1.5 px-0.5">
        <span class="eyebrow flex items-center gap-1">Workspace</span>
        <button
          type="button"
          class="text-muted hover:text-primary p-1 rounded-md hover:bg-surface-hover transition-colors cursor-pointer border-none bg-transparent"
          onclick={onShowEnvModal}
          title="Manage Environments"
          aria-label="Manage Environments"
        >
          <FolderPlus size={13} />
        </button>
      </div>
      <CustomSelect
        options={environments.map((env) => ({ value: env.name, label: env.name }))}
        value={activeEnv}
        onChange={(val) => onSwitchEnv(val)}
      />
    {:else}
      <div class="flex justify-center py-1">
        <button
          type="button"
          class="w-8 h-8 rounded-lg bg-surface-input border border-border flex items-center justify-center text-muted hover:text-accent hover:border-accent/40 transition-colors cursor-pointer"
          onclick={onShowEnvModal}
          title={`Profile: ${activeEnv}`}
          aria-label={`Profile: ${activeEnv}`}
        >
          <span class="text-[11px] font-bold uppercase">{activeEnv.slice(0, 2)}</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Primary Navigation Items -->
  <div class="flex-1 min-h-0 overflow-y-auto px-2 py-3 flex flex-col gap-1 scrollbar-none">
    {#if !sidebarCollapsed}
      <span class="eyebrow px-2 mb-1 text-[10px]">Navigation</span>
    {/if}

    <nav class="flex flex-col gap-0.5">
      {#each navItems as item}
        <button
          type="button"
          class="flex items-center gap-2.5 w-full py-2 px-2.5 rounded-lg cursor-pointer text-xs font-medium text-left transition-all duration-fast relative border
            {sidebarCollapsed ? 'justify-center px-0' : ''}
            {activeTab === item.tab
              ? 'border-accent/30 bg-accent/15 text-primary font-semibold shadow-sm'
              : 'border-transparent text-secondary hover:text-primary hover:bg-surface-hover'}"
          onclick={() => onTabChange(item.tab)}
          title={item.label}
          aria-label={item.label}
        >
          <item.icon
            size={16}
            class={activeTab === item.tab ? "text-accent" : "text-muted"}
          />
          {#if !sidebarCollapsed}
            <span class="truncate">{item.label}</span>
            {#if item.badge && item.badge > 0}
              <span class="ml-auto badge-pill bg-accent/20 text-accent border border-accent/30">
                {item.badge}
              </span>
            {/if}
          {/if}
        </button>
      {/each}

      {#if onShowSnippetsModal}
        <button
          type="button"
          class="flex items-center gap-2.5 w-full py-2 px-2.5 rounded-lg cursor-pointer text-xs font-medium text-left transition-all duration-fast border border-transparent text-secondary hover:text-primary hover:bg-surface-hover mt-1
            {sidebarCollapsed ? 'justify-center px-0' : ''}"
          onclick={onShowSnippetsModal}
          title="Command Snippets"
          aria-label="Command Snippets"
        >
          <Code size={16} class="text-warning/80" />
          {#if !sidebarCollapsed}
            <span>Snippets</span>
          {/if}
        </button>
      {/if}
    </nav>

    <!-- Tag Filter Chips (when tags exist and expanded) -->
    {#if !sidebarCollapsed && allTags.length > 0}
      <div class="mt-4 pt-3 border-t border-border/60">
        <span class="eyebrow px-2 mb-1.5 flex items-center gap-1">
          <Tag size={10} />
          Filter Tags
        </span>
        <div class="flex flex-wrap gap-1 px-1">
          <button
            type="button"
            class="text-[10px] py-0.5 px-2 rounded-md cursor-pointer transition-all border
              {selectedTag === null
                ? 'border-accent/40 bg-accent/20 text-accent font-semibold'
                : 'border-border bg-surface-input/60 text-muted hover:text-primary hover:border-border-hover'}"
            onclick={() => onTagSelect(null)}
          >
            All
          </button>
          {#each allTags as tag}
            <button
              type="button"
              class="text-[10px] py-0.5 px-2 rounded-md cursor-pointer transition-all border truncate max-w-[120px]
                {selectedTag === tag
                  ? 'border-accent/40 bg-accent/20 text-accent font-semibold'
                  : 'border-border bg-surface-input/60 text-muted hover:text-primary hover:border-border-hover'}"
              onclick={() => onTagSelect(tag)}
            >
              #{tag}
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <!-- Bottom Status Tray Dock -->
  <div class="p-2 border-t border-border/80 bg-surface-input/30 flex flex-col gap-1.5 shrink-0">
    {#if !sidebarCollapsed}
      <!-- Status Pills -->
      <div class="flex flex-col gap-1">
        <!-- SSH Agent -->
        <button
          type="button"
          class="flex items-center justify-between py-1.5 px-2.5 rounded-lg border border-border/70 bg-surface text-xs cursor-pointer transition-all hover:bg-surface-hover hover:border-border-hover"
          onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
          title="SSH Agent Status"
        >
          <span class="flex items-center gap-2 text-secondary text-[11px]">
            <KeyRound size={13} class={agentActive ? "text-running" : "text-muted"} />
            SSH Agent
          </span>
          <span class="font-mono text-[10px] font-semibold {agentActive ? 'text-running' : 'text-muted'}">
            {agentActive ? `${agentKeys.length} keys` : 'Inactive'}
          </span>
        </button>

        <!-- Kerberos GSSAPI -->
        {#if kerberosHealth !== "unavailable"}
          <button
            type="button"
            class="flex items-center justify-between py-1.5 px-2.5 rounded-lg border border-border/70 bg-surface text-xs cursor-pointer transition-all hover:bg-surface-hover hover:border-border-hover"
            onclick={onShowKerberosModal}
            title="Kerberos Ticket Status"
          >
            <span class="flex items-center gap-2 text-secondary text-[11px]">
              <ShieldCheck size={13} class={kerberosHealth === 'valid' ? 'text-running' : 'text-warning'} />
              Kerberos
            </span>
            <span class="font-mono text-[10px] font-semibold {kerberosHealth === 'valid' ? 'text-running' : 'text-warning'}">
              {kerberosHealth === 'valid' ? kerberosRemainingLabel : 'Ticket'}
            </span>
          </button>
        {/if}

        <!-- Detached / Away Sessions -->
        {#if externalSessionCount > 0}
          <button
            type="button"
            class="flex items-center justify-between py-1.5 px-2.5 rounded-lg border border-accent/40 bg-accent/15 text-accent text-xs font-semibold cursor-pointer hover:bg-accent/25 transition-all"
            onclick={onShowSessionManager}
            title="Background SSH Sessions"
          >
            <span class="flex items-center gap-1.5 text-[11px]">
              <Layers size={13} />
              Background
            </span>
            <span class="badge-pill bg-accent/30 text-accent font-bold">
              {externalSessionCount} away
            </span>
          </button>
        {/if}
      </div>
    {:else}
      <!-- Collapsed Status Icons -->
      <div class="flex flex-col items-center gap-1.5 py-1">
        <button
          type="button"
          class="w-7 h-7 rounded-md flex items-center justify-center text-muted hover:text-primary transition-colors cursor-pointer border-none bg-transparent"
          onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
          title={`Agent: ${agentActive ? `${agentKeys.length} keys` : 'Off'}`}
          aria-label="SSH Agent Status"
        >
          <KeyRound size={14} class={agentActive ? "text-running" : "text-muted"} />
        </button>
        {#if kerberosHealth !== "unavailable"}
          <button
            type="button"
            class="w-7 h-7 rounded-md flex items-center justify-center text-muted hover:text-primary transition-colors cursor-pointer border-none bg-transparent"
            onclick={onShowKerberosModal}
            title={`Kerberos: ${kerberosRemainingLabel}`}
            aria-label="Kerberos Status"
          >
            <ShieldCheck size={14} class={kerberosHealth === 'valid' ? "text-running" : "text-warning"} />
          </button>
        {/if}
        {#if externalSessionCount > 0}
          <button
            type="button"
            class="w-7 h-7 rounded-md flex items-center justify-center text-accent hover:bg-accent/20 transition-colors cursor-pointer border-none bg-accent/10"
            onclick={onShowSessionManager}
            title={`${externalSessionCount} background sessions`}
            aria-label="Background Sessions"
          >
            <Layers size={14} />
          </button>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Collapse Toggle Button -->
  <button
    type="button"
    class="absolute top-1/2 -right-3 -translate-y-1/2 bg-surface-raised border border-border-hover text-secondary cursor-pointer w-6 h-6 rounded-full flex items-center justify-center z-50 shadow-md transition-all hover:text-accent hover:border-accent"
    onclick={onToggleSidebar}
    title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
    aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
  >
    {#if sidebarCollapsed}
      <ChevronRight size={13} />
    {:else}
      <ChevronLeft size={13} />
    {/if}
  </button>
</aside>
