<script lang="ts">
  import {
    Check,
    ChevronsUpDown,
    Clock,
    Code,
    FolderCog,
    HardDrive,
    KeyRound,
    Layers,
    Network,
    PanelLeftClose,
    PanelLeftOpen,
    Server,
    Settings,
    ShieldCheck,
    Ticket,
    TerminalSquare,
  } from "lucide-svelte";
  import type { AppTab, DesktopSettings, EnvInfo } from "$lib/types";

  type KerberosHealth = "missing" | "expired" | "warning" | "valid" | "unavailable";

  interface Props {
    activeTab: AppTab;
    onTabChange: (tab: AppTab) => void;
    environments: EnvInfo[];
    activeEnv: string;
    onSwitchEnv: (name: string) => void;
    onShowEnvModal: () => void;
    sidebarCollapsed: boolean;
    onToggleSidebar: () => void;
    terminalCount: number;
    externalSessionCount: number;
    agentActive: boolean;
    agentKeys: string[];
    onStartAgent: () => void;
    onShowAgentModal: () => void;
    kerberosHealth: KerberosHealth;
    kerberosRemainingLabel: string;
    onShowKerberosModal: () => void;
    onShowSessionManager: () => void;
    onShowSnippetsModal: () => void;
    settings: DesktopSettings;
  }

  let {
    activeTab,
    onTabChange,
    environments,
    activeEnv,
    onSwitchEnv,
    onShowEnvModal,
    sidebarCollapsed,
    onToggleSidebar,
    terminalCount,
    externalSessionCount,
    agentActive,
    agentKeys,
    onStartAgent,
    onShowAgentModal,
    kerberosHealth,
    kerberosRemainingLabel,
    onShowKerberosModal,
    onShowSessionManager,
    onShowSnippetsModal,
    settings,
  }: Props = $props();

  type NavItem = { tab: AppTab; icon: typeof Server; label: string; badge?: number; key?: string };

  const sections = $derived<{ label: string | null; items: NavItem[] }[]>([
    {
      label: null,
      items: [
        { tab: "connections", icon: Server, label: "Hosts", key: "1" },
        { tab: "terminals", icon: TerminalSquare, label: "Terminals", badge: terminalCount, key: "2" },
        ...(settings.enable_sftp !== false ? [{ tab: "sftp" as AppTab, icon: HardDrive, label: "Files" }] : []),
        ...(settings.enable_tunneling !== false ? [{ tab: "tunnels" as AppTab, icon: Network, label: "Tunnels" }] : []),
      ],
    },
    {
      label: "Security",
      items: [
        { tab: "keys", icon: KeyRound, label: "Keys", key: "3" },
        { tab: "audit", icon: ShieldCheck, label: "Audit", key: "4" },
      ],
    },
    {
      label: "Activity",
      items: [{ tab: "history", icon: Clock, label: "History", key: "5" }],
    },
  ]);

  let profileMenuOpen = $state(false);
  let profileRoot = $state<HTMLDivElement>();

  function onWindowPointerDown(e: PointerEvent) {
    if (profileMenuOpen && profileRoot && !profileRoot.contains(e.target as Node)) {
      profileMenuOpen = false;
    }
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (profileMenuOpen && e.key === "Escape") {
      e.stopPropagation();
      profileMenuOpen = false;
    }
  }

  const kerberosTone = $derived(
    kerberosHealth === "valid" ? "success" : kerberosHealth === "warning" ? "warning" : "error",
  );
  const kerberosLabel = $derived(
    kerberosHealth === "valid" || kerberosHealth === "warning"
      ? kerberosRemainingLabel
      : kerberosHealth === "expired"
        ? "Expired"
        : "No ticket",
  );
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onWindowKeydown} />

<aside
  class="sidebar"
  style="width: {sidebarCollapsed ? 'var(--sidebar-w-collapsed)' : 'var(--sidebar-w)'}"
  aria-label="Main navigation"
>
  <!-- Profile switcher -->
  <div class="relative px-2.5 pb-1 pt-1" bind:this={profileRoot}>
    <button
      type="button"
      class="flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-md px-1.5 text-left transition-colors duration-fast hover:bg-surface-hover
        {sidebarCollapsed ? 'justify-center' : ''}"
      onclick={() => (profileMenuOpen = !profileMenuOpen)}
      aria-haspopup="menu"
      aria-expanded={profileMenuOpen}
      title={`Profile: ${activeEnv}`}
    >
      <span
        class="flex size-6 shrink-0 items-center justify-center rounded-md bg-surface-active text-2xs font-semibold uppercase text-primary"
      >
        {activeEnv.slice(0, 1)}
      </span>
      {#if !sidebarCollapsed}
        <span class="flex min-w-0 flex-1 flex-col leading-tight">
          <span class="truncate text-sm font-medium text-primary">{activeEnv}</span>
          <span class="text-2xs text-muted">Profile</span>
        </span>
        <ChevronsUpDown size={14} class="shrink-0 text-muted" />
      {/if}
    </button>

    {#if profileMenuOpen}
      <div
        class="popover absolute left-2.5 top-full mt-1 w-[220px]"
        role="menu"
      >
        <div class="section-label px-2 pb-1 pt-1.5">Profiles</div>
        {#each environments as env (env.name)}
          <button
            type="button"
            role="menuitemradio"
            aria-checked={env.name === activeEnv}
            class="menu-item"
            onclick={() => {
              profileMenuOpen = false;
              if (env.name !== activeEnv) onSwitchEnv(env.name);
            }}
          >
            <span class="flex size-5 items-center justify-center rounded-sm bg-surface-active text-2xs font-semibold uppercase">
              {env.name.slice(0, 1)}
            </span>
            <span class="truncate">{env.name}</span>
            {#if env.name === activeEnv}
              <Check size={14} class="ml-auto text-accent" />
            {/if}
          </button>
        {/each}
        <div class="menu-separator"></div>
        <button
          type="button"
          role="menuitem"
          class="menu-item"
          onclick={() => {
            profileMenuOpen = false;
            onShowEnvModal();
          }}
        >
          <FolderCog size={14} />
          Manage profiles…
        </button>
      </div>
    {/if}
  </div>

  <!-- Navigation -->
  <nav class="flex min-h-0 flex-1 flex-col overflow-y-auto px-2.5 pb-2 scrollbar-none">
    {#each sections as section}
      {#if section.label}
        {#if sidebarCollapsed}
          <div class="mx-2 my-2.5 h-px bg-border"></div>
        {:else}
          <div class="nav-section-label">{section.label}</div>
        {/if}
      {:else}
        <div class="h-2"></div>
      {/if}
      <div class="nav-section">
        {#each section.items as item (item.tab)}
          {@const active = activeTab === item.tab}
          <button
            type="button"
            class="nav-item {active ? 'nav-item-active' : ''} {sidebarCollapsed ? 'nav-item-collapsed' : ''}"
            onclick={() => onTabChange(item.tab)}
            title={sidebarCollapsed ? item.label : item.key ? `${item.label} (${item.key})` : undefined}
            aria-label={item.label}
            aria-current={active ? "page" : undefined}
          >
            <item.icon size={16} strokeWidth={1.75} class={active ? "text-primary" : "text-muted"} />
            {#if !sidebarCollapsed}
              <span class="truncate">{item.label}</span>
              {#if item.badge}
                <span class="count ml-auto {active ? 'bg-accent/15 text-accent' : ''}">{item.badge}</span>
              {/if}
            {:else if item.badge}
              <span class="absolute right-1.5 top-1.5 size-1.5 rounded-full bg-accent"></span>
            {/if}
          </button>
        {/each}
        {#if section.label === "Activity"}
          <button
            type="button"
            class="nav-item {sidebarCollapsed ? 'nav-item-collapsed' : ''}"
            onclick={onShowSnippetsModal}
            title="Snippets"
            aria-label="Snippets"
          >
            <Code size={16} strokeWidth={1.75} class="text-muted" />
            {#if !sidebarCollapsed}<span>Snippets</span>{/if}
          </button>
        {/if}
      </div>
    {/each}
  </nav>

  <!-- Status + settings -->
  <div class="flex flex-col gap-px px-2.5 pb-2.5">
    {#if externalSessionCount > 0}
      <button
        type="button"
        class="nav-item {sidebarCollapsed ? 'nav-item-collapsed' : ''}"
        onclick={onShowSessionManager}
        title="Background sessions"
        aria-label="Background sessions"
      >
        <Layers size={16} strokeWidth={1.75} class="text-accent" />
        {#if !sidebarCollapsed}
          <span class="truncate">Background</span>
          <span class="count ml-auto bg-accent/15 text-accent">{externalSessionCount}</span>
        {/if}
      </button>
    {/if}

    <button
      type="button"
      class="nav-item {sidebarCollapsed ? 'nav-item-collapsed' : ''}"
      onclick={() => (agentActive ? onShowAgentModal() : onStartAgent())}
      title={agentActive ? `SSH agent · ${agentKeys.length} keys loaded` : "SSH agent stopped — click to start"}
      aria-label="SSH agent"
    >
      <span class="relative flex size-4 items-center justify-center">
        <KeyRound size={16} strokeWidth={1.75} class="text-muted" />
        <span class="status-dot status-dot-sm absolute -bottom-0.5 -right-0.5 ring-2 ring-chrome {agentActive ? 'status-dot-success' : 'status-dot-offline'}"></span>
      </span>
      {#if !sidebarCollapsed}
        <span class="truncate">SSH agent</span>
        <span class="ml-auto text-xs tabular-nums text-muted">
          {agentActive ? `${agentKeys.length} key${agentKeys.length === 1 ? "" : "s"}` : "Off"}
        </span>
      {/if}
    </button>

    {#if kerberosHealth !== "unavailable"}
      <button
        type="button"
        class="nav-item {sidebarCollapsed ? 'nav-item-collapsed' : ''}"
        onclick={onShowKerberosModal}
        title={`Kerberos · ${kerberosLabel}`}
        aria-label="Kerberos ticket"
      >
        <span class="relative flex size-4 items-center justify-center">
          <Ticket size={16} strokeWidth={1.75} class="text-muted" />
          <span class="status-dot status-dot-sm absolute -bottom-0.5 -right-0.5 ring-2 ring-chrome status-dot-{kerberosTone}"></span>
        </span>
        {#if !sidebarCollapsed}
          <span class="truncate">Kerberos</span>
          <span class="ml-auto truncate text-xs tabular-nums {kerberosTone === 'success' ? 'text-muted' : `text-${kerberosTone}`}">
            {kerberosLabel}
          </span>
        {/if}
      </button>
    {/if}

    <div class="mx-1 my-1.5 h-px bg-border"></div>

    <div class="flex items-center gap-px {sidebarCollapsed ? 'flex-col' : ''}">
      <button
        type="button"
        class="nav-item flex-1 {activeTab === 'settings' ? 'nav-item-active' : ''} {sidebarCollapsed ? 'nav-item-collapsed' : ''}"
        onclick={() => onTabChange("settings")}
        title="Settings (6)"
        aria-label="Settings"
        aria-current={activeTab === "settings" ? "page" : undefined}
      >
        <Settings size={16} strokeWidth={1.75} class={activeTab === "settings" ? "text-primary" : "text-muted"} />
        {#if !sidebarCollapsed}<span>Settings</span>{/if}
      </button>
      <button
        type="button"
        class="btn-icon size-8"
        onclick={onToggleSidebar}
        title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
      >
        {#if sidebarCollapsed}
          <PanelLeftOpen size={16} strokeWidth={1.75} />
        {:else}
          <PanelLeftClose size={16} strokeWidth={1.75} />
        {/if}
      </button>
    </div>
  </div>
</aside>
