<script lang="ts">
  import {
    Layers,
    KeyRound,
    ShieldCheck,
    TerminalSquare,
    FileText,
    Palette,
    Sliders,
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
</script>

<div class="flex flex-1 min-h-0 w-full overflow-hidden bg-surface">
  <!-- Settings Sidebar -->
  <div class="w-60 min-w-60 border-r border-border flex flex-col shrink-0 bg-surface">
    <div class="px-5 pt-6 pb-4">
      <h2 class="text-xs font-bold tracking-widest text-muted uppercase">Settings</h2>
    </div>
    <div class="flex-1 overflow-y-auto px-2.5 pb-4 flex flex-col gap-0.5 select-none">
      <button
        type="button"
        class="flex items-center gap-3 px-3 py-2.5 rounded-lg text-[13px] font-medium cursor-pointer transition-all duration-100 text-left outline-none
          {activeCategory === 'workspace' ? 'bg-white/[0.04] text-primary font-semibold' : 'text-secondary bg-transparent hover:bg-white/[0.02] hover:text-primary'}"
        onclick={() => (activeCategory = "workspace")}
      >
        <Layers size={15} class="text-muted" />
        <span>Profiles & Workspace</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 px-3 py-2.5 rounded-lg text-[13px] font-medium cursor-pointer transition-all duration-100 text-left outline-none
          {activeCategory === 'ssh_agent' ? 'bg-white/[0.04] text-primary font-semibold' : 'text-secondary bg-transparent hover:bg-white/[0.02] hover:text-primary'}"
        onclick={() => (activeCategory = "ssh_agent")}
      >
        <KeyRound size={15} class="text-muted" />
        <span>SSH Agent & Defaults</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 px-3 py-2.5 rounded-lg text-[13px] font-medium cursor-pointer transition-all duration-100 text-left outline-none
          {activeCategory === 'kerberos' ? 'bg-white/[0.04] text-primary font-semibold' : 'text-secondary bg-transparent hover:bg-white/[0.02] hover:text-primary'}"
        onclick={() => (activeCategory = "kerberos")}
      >
        <ShieldCheck size={15} class="text-muted" />
        <span>Kerberos GSSAPI</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 px-3 py-2.5 rounded-lg text-[13px] font-medium cursor-pointer transition-all duration-100 text-left outline-none
          {activeCategory === 'terminal' ? 'bg-white/[0.04] text-primary font-semibold' : 'text-secondary bg-transparent hover:bg-white/[0.02] hover:text-primary'}"
        onclick={() => (activeCategory = "terminal")}
      >
        <TerminalSquare size={15} class="text-muted" />
        <span>Terminal Emulation</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 px-3 py-2.5 rounded-lg text-[13px] font-medium cursor-pointer transition-all duration-100 text-left outline-none
          {activeCategory === 'logs' ? 'bg-white/[0.04] text-primary font-semibold' : 'text-secondary bg-transparent hover:bg-white/[0.02] hover:text-primary'}"
        onclick={() => (activeCategory = "logs")}
      >
        <FileText size={15} class="text-muted" />
        <span>Session Logs</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 px-3 py-2.5 rounded-lg text-[13px] font-medium cursor-pointer transition-all duration-100 text-left outline-none
          {activeCategory === 'appearance' ? 'bg-white/[0.04] text-primary font-semibold' : 'text-secondary bg-transparent hover:bg-white/[0.02] hover:text-primary'}"
        onclick={() => (activeCategory = "appearance")}
      >
        <Palette size={15} class="text-muted" />
        <span>Appearance & Locale</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 px-3 py-2.5 rounded-lg text-[13px] font-medium cursor-pointer transition-all duration-100 text-left outline-none
          {activeCategory === 'features' ? 'bg-white/[0.04] text-primary font-semibold' : 'text-secondary bg-transparent hover:bg-white/[0.02] hover:text-primary'}"
        onclick={() => (activeCategory = "features")}
      >
        <Sliders size={15} class="text-muted" />
        <span>Features</span>
      </button>
    </div>
  </div>

  <!-- Settings Content Panel -->
  <div class="flex-1 min-h-0 overflow-y-auto bg-surface/30 px-8 py-7 overscroll-contain">
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
