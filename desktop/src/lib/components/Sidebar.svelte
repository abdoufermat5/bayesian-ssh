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
  class="sidebar-shell"
  style="width: {sidebarCollapsed ? 'var(--sidebar-w-collapsed)' : 'var(--sidebar-w)'};"
>
  <!-- Top Profile / Workspace Header -->
  <div class="p-2.5 border-b border-border shrink-0">
    {#if !sidebarCollapsed}
      <div class="flex items-center justify-between gap-1 mb-1.5 px-1">
        <span class="eyebrow text-[10px]">Workspace</span>
        <button
          type="button"
          class="btn-icon p-1 h-6 w-6"
          onclick={onShowEnvModal}
          title="Manage Environments"
          aria-label="Manage Environments"
        >
          <FolderPlus size={12} />
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
          class="icon-tile text-xs font-bold uppercase hover:border-border-hover"
          onclick={onShowEnvModal}
          title={`Profile: ${activeEnv}`}
          aria-label={`Profile: ${activeEnv}`}
        >
          <span class="text-[10px] font-mono font-semibold">{activeEnv.slice(0, 2).toUpperCase()}</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Primary Navigation Items -->
  <div class="flex-1 min-h-0 overflow-y-auto px-2 py-2.5 flex flex-col gap-0.5 scrollbar-none">
    {#if !sidebarCollapsed}
      <span class="eyebrow px-2 mb-1 text-[10px]">Views</span>
    {/if}

    <nav class="flex flex-col gap-0.5">
      {#each navItems as item}
        <button
          type="button"
          class="nav-item
            {sidebarCollapsed ? 'justify-center px-0' : ''}
            {activeTab === item.tab ? 'nav-item-active' : ''}"
          onclick={() => onTabChange(item.tab)}
          title={item.label}
          aria-label={item.label}
        >
          <item.icon
            size={15}
            class={activeTab === item.tab ? "text-primary" : "text-muted"}
          />
          {#if !sidebarCollapsed}
            <span class="truncate">{item.label}</span>
            {#if item.badge && item.badge > 0}
              <span class="ml-auto px-1.5 py-0.2 rounded-full font-mono text-[10px] font-medium bg-white/10 text-primary">
                {item.badge}
              </span>
            {/if}
          {/if}
        </button>
      {/each}

      {#if onShowSnippetsModal}
        <button
          type="button"
          class="nav-item mt-1 text-muted
            {sidebarCollapsed ? 'justify-center px-0' : ''}"
          onclick={onShowSnippetsModal}
          title="Command Snippets"
          aria-label="Command Snippets"
        >
          <Code size={15} />
          {#if !sidebarCollapsed}
            <span>Snippets</span>
          {/if}
        </button>
      {/if}
    </nav>

    <!-- Tag Filter Chips (when tags exist and expanded) -->
    {#if !sidebarCollapsed && allTags.length > 0}
      <div class="mt-3 pt-2.5 border-t border-border/60">
        <span class="eyebrow px-2 mb-1.5 flex items-center gap-1">
          <Tag size={10} />
          Tags
        </span>
        <div class="flex flex-wrap gap-1 px-1 max-h-32 overflow-y-auto scrollbar-none">
          <button
            type="button"
            class="filter-chip
              {selectedTag === null ? 'filter-chip-active' : 'filter-chip-idle'}"
            onclick={() => onTagSelect(null)}
          >
            All
          </button>
          {#each allTags as tag}
            <button
              type="button"
              class="filter-chip truncate max-w-[110px]
                {selectedTag === tag ? 'filter-chip-active' : 'filter-chip-idle'}"
              onclick={() => onTagSelect(tag)}
              title={`#${tag}`}
            >
              #{tag}
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <!-- Bottom System Status Tray -->
  <div class="p-2 border-t border-border bg-surface-input/30 flex flex-col gap-1 shrink-0">
    {#if !sidebarCollapsed}
      <div class="flex flex-col gap-0.5">
        <!-- SSH Agent -->
        <button
          type="button"
          class="status-card"
          onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
          title="SSH Agent Status"
        >
          <span class="flex items-center gap-2 text-secondary text-xs">
            <span class="status-dot status-dot-sm {agentActive ? 'bg-success' : 'bg-muted/40'}"></span>
            SSH Agent
          </span>
          <span class="font-mono text-[10px] text-muted">
            {agentActive ? `${agentKeys.length} keys` : 'Inactive'}
          </span>
        </button>

        <!-- Kerberos GSSAPI -->
        {#if kerberosHealth !== "unavailable"}
          <button
            type="button"
            class="status-card"
            onclick={onShowKerberosModal}
            title="Kerberos Ticket Status"
          >
            <span class="flex items-center gap-2 text-secondary text-xs">
              <span class="status-dot status-dot-sm {kerberosHealth === 'valid' ? 'bg-success' : 'bg-warning'}"></span>
              Kerberos
            </span>
            <span class="font-mono text-[10px] text-muted truncate max-w-[80px]">
              {kerberosHealth === 'valid' ? kerberosRemainingLabel : 'Ticket'}
            </span>
          </button>
        {/if}

        <!-- Detached / Away Sessions -->
        {#if externalSessionCount > 0}
          <button
            type="button"
            class="status-card border border-border bg-surface-raised font-medium text-primary hover:bg-surface-hover"
            onclick={onShowSessionManager}
            title="Background SSH Sessions"
          >
            <span class="flex items-center gap-1.5 text-xs">
              <Layers size={12} class="text-accent" />
              Background
            </span>
            <span class="px-1.5 py-0.2 rounded-full font-mono text-[10px] bg-accent/15 text-accent">
              {externalSessionCount} away
            </span>
          </button>
        {/if}
      </div>

      <!-- Collapse button in footer row -->
      <div class="flex items-center justify-between pt-1 border-t border-border/40 mt-1 px-1">
        <span class="text-[10px] font-mono text-muted/60">bssh v2.5.2</span>
        <button
          type="button"
          class="btn-icon h-6 w-6 p-0 text-muted hover:text-primary"
          onclick={onToggleSidebar}
          title="Collapse sidebar"
          aria-label="Collapse sidebar"
        >
          <ChevronLeft size={13} />
        </button>
      </div>
    {:else}
      <!-- Collapsed Status Icons -->
      <div class="flex flex-col items-center gap-1 py-1">
        <button
          type="button"
          class="btn-icon h-7 w-7 p-0"
          onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
          title={`Agent: ${agentActive ? `${agentKeys.length} keys` : 'Off'}`}
          aria-label="SSH Agent Status"
        >
          <KeyRound size={13} class={agentActive ? "text-success" : "text-muted"} />
        </button>
        {#if kerberosHealth !== "unavailable"}
          <button
            type="button"
            class="btn-icon h-7 w-7 p-0"
            onclick={onShowKerberosModal}
            title={`Kerberos: ${kerberosRemainingLabel}`}
            aria-label="Kerberos Status"
          >
            <ShieldCheck size={13} class={kerberosHealth === 'valid' ? "text-success" : "text-warning"} />
          </button>
        {/if}
        {#if externalSessionCount > 0}
          <button
            type="button"
            class="btn-icon h-7 w-7 bg-surface-raised p-0 text-accent hover:bg-surface-hover"
            onclick={onShowSessionManager}
            title={`${externalSessionCount} background sessions`}
            aria-label="Background Sessions"
          >
            <Layers size={13} />
          </button>
        {/if}
        <button
          type="button"
          class="btn-icon h-7 w-7 p-0 mt-1 border-t border-border/40 text-muted hover:text-primary"
          onclick={onToggleSidebar}
          title="Expand sidebar"
          aria-label="Expand sidebar"
        >
          <ChevronRight size={13} />
        </button>
      </div>
    {/if}
  </div>
</aside>
