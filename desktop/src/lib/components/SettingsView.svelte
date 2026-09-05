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

<div class="flex flex-1 min-h-0 w-full overflow-hidden bg-surface select-none">
  <!-- Settings Sidebar -->
  <div class="w-64 min-w-64 border-r border-border flex flex-col shrink-0 bg-surface-input/20">
    <div class="px-5 pt-6 pb-3">
      <span class="eyebrow text-[10px]">Preferences</span>
      <h2 class="text-base font-bold tracking-tight text-primary mt-0.5">Settings</h2>
    </div>

    <div class="flex-1 overflow-y-auto px-3 pb-4 flex flex-col gap-1 select-none scrollbar-none">
      {#each categories as cat}
        <button
          type="button"
          class="flex items-center gap-3 px-3 py-2.5 rounded-xl text-xs font-medium cursor-pointer transition-all duration-fast text-left outline-none border
            {activeCategory === cat.id
              ? 'border-accent/35 bg-accent/15 text-primary font-semibold shadow-sm'
              : 'border-transparent text-secondary bg-transparent hover:bg-surface-hover hover:text-primary'}"
          onclick={() => (activeCategory = cat.id)}
        >
          <cat.icon size={15} class={activeCategory === cat.id ? "text-accent" : "text-muted"} />
          <span class="truncate">{cat.label}</span>
        </button>
      {/each}
    </div>
  </div>

  <!-- Settings Content Panel -->
  <div class="flex-1 min-h-0 overflow-y-auto bg-surface px-8 py-7 overscroll-contain scrollbar-none">
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
