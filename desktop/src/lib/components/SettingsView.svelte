<script lang="ts">
  import { Layers, Lock, Network, Palette, RefreshCw, TerminalSquare } from "lucide-svelte";
  import type { DesktopSettings, EnvInfo, WorkspaceInfo } from "$lib/types";
  import {
    SETTINGS_SECTIONS,
    pendingSettingsSection,
    takeSettingsSection,
    type SettingsSectionId,
  } from "$lib/stores/settingsNav.svelte";
  import ProfilesSettings from "./settings/ProfilesSettings.svelte";
  import AgentSettings from "./settings/AgentSettings.svelte";
  import KerberosSettings from "./settings/KerberosSettings.svelte";
  import TerminalSettings from "./settings/TerminalSettings.svelte";
  import LogsSettings from "./settings/LogsSettings.svelte";
  import RecordingSettings from "./settings/RecordingSettings.svelte";
  import AppearanceSettings from "./settings/AppearanceSettings.svelte";
  import SectionToggles from "./settings/SectionToggles.svelte";
  import SourcesSettings from "./settings/SourcesSettings.svelte";
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

  const ICONS = {
    workspace: Layers,
    connection: Network,
    terminal: TerminalSquare,
    privacy: Lock,
    appearance: Palette,
    sources: RefreshCw,
  } as const;

  const LEADS: Record<SettingsSectionId, string> = {
    workspace: "Environment profiles, host search and where your data lives.",
    connection: "SSH agent, Kerberos tickets and the values used when a host doesn't set its own.",
    terminal: "Typography, cursor and behaviour of terminal sessions, file browsing and optional sections.",
    privacy: "What the app keeps about your sessions, and how much it logs.",
    appearance: "Theme, density and how dates are displayed.",
    sources: "Where hosts are imported from, and this installation's version and update channel.",
  };

  // A request from elsewhere (command palette, a status link) is taken on
  // mount and whenever a new one arrives while this view is open.
  let active = $state<SettingsSectionId>(takeSettingsSection() ?? "workspace");
  let content = $state<HTMLElement>();

  $effect(() => {
    if (pendingSettingsSection()) {
      const id = takeSettingsSection();
      if (id) select(id);
    }
  });

  function select(id: SettingsSectionId) {
    active = id;
    content?.scrollTo({ top: 0 });
  }

  const current = $derived(SETTINGS_SECTIONS.find((s) => s.id === active) ?? SETTINGS_SECTIONS[0]);
</script>

<div class="view">
  <div class="settings-frame">
    <header class="view-header">
      <div class="flex min-w-0 items-baseline gap-2.5">
        <span class="settings-eyebrow" aria-hidden="true">Settings /</span>
        <h1 class="view-title">{current.label}</h1>
      </div>
    </header>

    <div class="settings-shell">
      <nav class="settings-sidebar" aria-label="Settings sections">
        {#each SETTINGS_SECTIONS as sec (sec.id)}
          {@const active_ = active === sec.id}
          {@const Icon = ICONS[sec.id]}
          <button
            type="button"
            class="settings-nav-item {active_ ? 'settings-nav-item-active' : ''}"
            onclick={() => select(sec.id)}
            title={sec.label}
            aria-label={sec.label}
            aria-current={active_ ? "page" : undefined}
          >
            <Icon size={16} class="shrink-0 {active_ ? 'text-primary' : 'text-muted'}" aria-hidden="true" />
            <span class="settings-label truncate">{sec.label}</span>
          </button>
        {/each}
      </nav>

      <div class="settings-content" bind:this={content}>
        <div class="settings-stack">
          <p class="settings-lead">{LEADS[active]}</p>

          {#if active === "workspace"}
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
          {:else if active === "connection"}
            <AgentSettings bind:settings bind:workspace {onSave} {onSaveWorkspace} />
            <KerberosSettings bind:settings {onSave} />
          {:else if active === "terminal"}
            <TerminalSettings bind:settings {onSave} />
            <SectionToggles bind:settings {onSave} />
          {:else if active === "privacy"}
            <LogsSettings bind:settings bind:workspace {onSaveWorkspace} />
            <RecordingSettings bind:settings {onSave} />
          {:else if active === "appearance"}
            <AppearanceSettings bind:settings {onSave} {onThemeChange} />
          {:else if active === "sources"}
            <SourcesSettings />
            <UpdatesSettings />
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
