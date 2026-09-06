<script lang="ts">
  import {
    FileText,
    KeyRound,
    Layers,
    Palette,
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
    { id: "workspace", label: "Profiles & Workspace", icon: Layers },
    { id: "ssh_agent", label: "SSH Agent & Defaults", icon: KeyRound },
    { id: "kerberos", label: "Kerberos GSSAPI", icon: ShieldCheck },
    { id: "terminal", label: "Terminal Emulation", icon: TerminalSquare },
    { id: "logs", label: "Session Logs", icon: FileText },
    { id: "appearance", label: "Appearance & Locale", icon: Palette },
    { id: "features", label: "Features Flags", icon: Sliders },
  ];
</script>

<div class="settings-shell">
  <div class="settings-sidebar">
    <div class="settings-sidebar-title px-4 pt-5 pb-3">
      <span class="eyebrow text-[10px]">Preferences</span>
      <h2 class="text-base font-bold tracking-tight text-primary mt-0.5">Settings</h2>
    </div>

    <div class="settings-nav">
      {#each categories as cat}
        <button
          type="button"
          class="settings-nav-item
            {activeCategory === cat.id
              ? 'settings-nav-item-active'
              : ''}"
          onclick={() => (activeCategory = cat.id)}
          title={cat.label}
          aria-current={activeCategory === cat.id ? "page" : undefined}
        >
          <cat.icon size={15} class={activeCategory === cat.id ? "text-accent" : "text-muted"} />
          <span class="settings-label">{cat.label}</span>
        </button>
      {/each}
    </div>
  </div>

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
    {/if}

    {#if activeCategory === "ssh_agent"}
      <AgentSettings bind:settings bind:workspace {onSave} {onSaveWorkspace} />
    {/if}

    {#if activeCategory === "kerberos"}
      <KerberosSettings bind:settings {onSave} />
    {/if}

    {#if activeCategory === "terminal"}
      <TerminalSettings bind:settings {onSave} />
    {/if}

    {#if activeCategory === "logs"}
      <LogsSettings bind:settings bind:workspace {onSaveWorkspace} />
    {/if}

    {#if activeCategory === "appearance"}
      <AppearanceSettings bind:settings {onSave} {onThemeChange} />
    {/if}

    {#if activeCategory === "features"}
      <FeaturesSettings bind:settings {onSave} />
    {/if}
  </div>
</div>
