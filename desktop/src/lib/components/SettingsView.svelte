<script lang="ts">
  import {
    History,
    KeyRound,
    Layers,
    Palette,
    RefreshCw,
    ShieldCheck,
    Sliders,
    TerminalSquare,
  } from "lucide-svelte";
  import type { DesktopSettings, EnvInfo, WorkspaceInfo } from "$lib/types";
  import ProfilesSettings from "./settings/ProfilesSettings.svelte";
  import AgentSettings from "./settings/AgentSettings.svelte";
  import KerberosSettings from "./settings/KerberosSettings.svelte";
  import TerminalSettings from "./settings/TerminalSettings.svelte";
  import LogsSettings from "./settings/LogsSettings.svelte";
  import AppearanceSettings from "./settings/AppearanceSettings.svelte";
  import FeaturesSettings from "./settings/FeaturesSettings.svelte";
  import UpdatesSettings from "./settings/UpdatesSettings.svelte";

  interface Props {
    settings: DesktopSettings;
    workspace: WorkspaceInfo;
    environments: EnvInfo[];
    onSave: () => void;
    onThemeChange: (theme: string) => void;
    onSaveWorkspace: () => void;
    onSwitchEnv: (name: string) => void;
    onManageProfiles: () => void;
    onBrowseSshConfig: () => void;
    onImportSshConfig: () => void;
  }

  let {
    settings = $bindable(),
    workspace = $bindable(),
    environments,
    onSave,
    onThemeChange,
    onSaveWorkspace,
    onSwitchEnv,
    onManageProfiles,
    onBrowseSshConfig,
    onImportSshConfig,
  }: Props = $props();

  let activeCategory = $state("workspace");

  const categories = [
    { id: "workspace", label: "Profiles & workspace", icon: Layers },
    { id: "ssh_agent", label: "SSH agent", icon: KeyRound },
    { id: "kerberos", label: "Kerberos", icon: ShieldCheck },
    { id: "terminal", label: "Terminal", icon: TerminalSquare },
    { id: "logs", label: "History", icon: History },
    { id: "appearance", label: "Appearance", icon: Palette },
    { id: "features", label: "Features", icon: Sliders },
    { id: "updates", label: "Updates", icon: RefreshCw },
  ];
</script>

<div class="view">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Settings</h1>
    </div>
  </header>

  <div class="settings-shell bg-surface">
    <nav class="settings-sidebar" aria-label="Settings sections">
    {#each categories as cat (cat.id)}
      {@const active = activeCategory === cat.id}
      <button
        type="button"
        class="settings-nav-item {active ? 'settings-nav-item-active' : ''}"
        onclick={() => (activeCategory = cat.id)}
        title={cat.label}
        aria-current={active ? "page" : undefined}
      >
        <cat.icon size={16} class="shrink-0 {active ? 'text-primary' : 'text-muted'}" />
        <span class="settings-label truncate">{cat.label}</span>
      </button>
    {/each}
  </nav>

  <div class="settings-content">
    {#if activeCategory === "workspace"}
      <ProfilesSettings
        bind:settings
        bind:workspace
        {environments}
        {onSave}
        {onSaveWorkspace}
        {onSwitchEnv}
        {onManageProfiles}
        {onBrowseSshConfig}
        {onImportSshConfig}
      />
    {:else if activeCategory === "ssh_agent"}
      <AgentSettings bind:settings bind:workspace {onSave} {onSaveWorkspace} />
    {:else if activeCategory === "kerberos"}
      <KerberosSettings bind:settings {onSave} />
    {:else if activeCategory === "terminal"}
      <TerminalSettings bind:settings {onSave} />
    {:else if activeCategory === "logs"}
      <LogsSettings bind:settings bind:workspace {onSaveWorkspace} />
    {:else if activeCategory === "appearance"}
      <AppearanceSettings bind:settings {onSave} {onThemeChange} />
    {:else if activeCategory === "features"}
      <FeaturesSettings bind:settings {onSave} />
    {:else if activeCategory === "updates"}
      <UpdatesSettings />
    {/if}
  </div>
  </div>
</div>
