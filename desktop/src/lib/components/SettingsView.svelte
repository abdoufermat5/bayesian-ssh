<script lang="ts">
  import {
    Folder,
    KeyRound,
    ShieldCheck,
    FileText,
    Palette,
    FolderPlus,
    FolderOpen,
    RefreshCw,
    Layers,
    Database,
  } from "lucide-svelte";
  import type { DesktopSettings, EnvInfo, WorkspaceInfo } from "$lib/types";
  import {
    SYSTEM_TIMEZONE,
    getSupportedTimezones,
    getSystemTimezone,
  } from "$lib/utils/timezone";

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
  let sshConfigPath = $state(workspace.ssh_config_path || "");
  const systemTimezone = getSystemTimezone();
  const timezoneOptions = getSupportedTimezones();
  let timezoneFilter = $state("");

  const filteredTimezoneOptions = $derived.by(() => {
    const query = timezoneFilter.trim().toLowerCase();
    const selected = settings.timezone;
    const base = !query
      ? timezoneOptions
      : timezoneOptions.filter((tz) => tz.toLowerCase().includes(query));

    if (selected && selected !== SYSTEM_TIMEZONE && !base.includes(selected)) {
      return [selected, ...base];
    }

    return base;
  });

  $effect(() => {
    sshConfigPath = workspace.ssh_config_path || "";
  });

  function syncWorkspaceFromForm() {
    workspace = {
      ...workspace,
      ssh_config_path: sshConfigPath.trim() || null,
      search_mode: settings.fuzzy_search ? "fuzzy" : "bayesian",
      default_user: settings.default_user,
      default_port: settings.default_port,
    };
  }

  function handleWorkspaceSave() {
    syncWorkspaceFromForm();
    onSaveWorkspace();
  }

  function handleDefaultsSave() {
    syncWorkspaceFromForm();
    onSave();
    onSaveWorkspace();
  }
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
    </div>
  </div>

  <!-- Settings Content Panel -->
  <div class="flex-1 min-h-0 overflow-y-auto bg-surface/30 px-8 py-7 overscroll-contain">
    {#if activeCategory === "workspace"}
      <div class="flex flex-col gap-6 max-w-2xl">
        <div>
          <h3 class="text-base font-semibold text-primary m-0">Profiles & Workspace</h3>
          <p class="text-xs text-muted mt-1">Configure active environment profiles and file paths</p>
        </div>

        <div class="h-px bg-border/50"></div>

        <div class="flex flex-col gap-1.5">
          <label for="settings-profile" class="text-xs font-semibold text-secondary">Active Profile</label>
          <span class="text-[11px] text-muted">Hosts and credentials are isolated within environment profiles</span>
          <div class="flex gap-2 mt-1">
            <select
              id="settings-profile"
              class="flex-1 bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] cursor-pointer transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
              value={workspace.active_env}
              onchange={(e) => onSwitchEnv((e.target as HTMLSelectElement).value)}
            >
              {#each environments as env}
                <option value={env.name}>{env.name}</option>
              {/each}
            </select>
            <button
              type="button"
              class="bg-white/[0.04] border border-border text-secondary py-2 px-3.5 rounded-lg cursor-pointer font-semibold flex items-center gap-1.5 text-xs whitespace-nowrap transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.06] outline-none"
              onclick={onManageProfiles}
            >
              <FolderPlus size={14} />
              Manage Profiles
            </button>
          </div>
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="settings-ssh-config" class="text-xs font-semibold text-secondary">OpenSSH Config Path</label>
          <span class="text-[11px] text-muted">Path to your OpenSSH configuration file for host importing</span>
          <div class="flex gap-2 mt-1">
            <input
              id="settings-ssh-config"
              type="text"
              placeholder="~/.ssh/config"
              value={sshConfigPath}
              onchange={handleWorkspaceSave}
              class="flex-1 bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
            />
            <button
              type="button"
              class="bg-white/[0.04] border border-border text-secondary py-2 px-3.5 rounded-lg cursor-pointer font-semibold flex items-center gap-1.5 text-xs whitespace-nowrap transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.06] outline-none"
              onclick={onBrowseSshConfig}
            >
              <FolderOpen size={14} />
              Browse
            </button>
          </div>
          <div class="flex gap-2 mt-1.5">
            <button
              type="button"
              class="bg-white/[0.04] border border-border text-secondary py-1.5 px-3 rounded-lg cursor-pointer font-semibold flex items-center gap-1.5 text-xs whitespace-nowrap transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.06] outline-none"
              onclick={onImportSshConfig}
            >
              <RefreshCw size={12} />
              Import hosts now
            </button>
          </div>
        </div>

        <div class="flex items-center justify-between gap-4 py-2">
          <div class="flex flex-col gap-0.5">
            <span class="text-xs font-semibold text-secondary">Host Ranking Mode</span>
            <span class="text-[11px] text-muted">Bayesian uses frequency + recency; fuzzy uses text matching</span>
          </div>
          <select
            class="min-w-[180px] bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] cursor-pointer transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
            value={settings.fuzzy_search ? "fuzzy" : "bayesian"}
            onchange={(e) => {
              settings.fuzzy_search = (e.target as HTMLSelectElement).value === "fuzzy";
              handleDefaultsSave();
            }}
          >
            <option value="bayesian">Bayesian ranking</option>
            <option value="fuzzy">Fuzzy search</option>
          </select>
        </div>

        <div class="h-px bg-border/50 my-1"></div>

        <div class="flex flex-col gap-3">
          <h4 class="text-[10px] font-bold tracking-widest text-muted uppercase flex items-center gap-2">
            <Database size={12} />
            <span>Workspace System Paths</span>
          </h4>
          <div class="grid grid-cols-[140px_1fr] gap-x-4 gap-y-3.5 items-center text-xs text-secondary mt-1">
            <span class="text-muted font-medium">Config root</span>
            <div class="bg-surface-input border border-border px-3 py-2 rounded-lg break-all">
              <code class="font-mono text-[11px] leading-normal">{workspace.config_root}</code>
            </div>
            <span class="text-muted font-medium">Profile directory</span>
            <div class="bg-surface-input border border-border px-3 py-2 rounded-lg break-all">
              <code class="font-mono text-[11px] leading-normal">{workspace.env_dir}</code>
            </div>
            <span class="text-muted font-medium">Database path</span>
            <div class="bg-surface-input border border-border px-3 py-2 rounded-lg break-all">
              <code class="font-mono text-[11px] leading-normal">{workspace.database_path}</code>
            </div>
          </div>
        </div>
      </div>
    {/if}

    {#if activeCategory === "ssh_agent"}
      <div class="flex flex-col gap-6 max-w-2xl">
        <div>
          <h3 class="text-base font-semibold text-primary m-0">SSH Agent & Defaults</h3>
          <p class="text-xs text-muted mt-1">Configure SSH Agent options and fallback connection values</p>
        </div>

        <div class="h-px bg-border/50"></div>

        <div class="flex items-center justify-between gap-4 py-1">
          <div class="flex flex-col gap-0.5">
            <span class="text-xs font-semibold text-secondary">Auto-start SSH Agent</span>
            <span class="text-[11px] text-muted leading-snug">Automatically launch internal agent on desktop app startup</span>
          </div>
          <input
            type="checkbox"
            checked={settings.auto_start_agent}
            onchange={(e) => {
              settings.auto_start_agent = (e.target as HTMLInputElement).checked;
              onSave();
            }}
            class="w-[18px] h-[18px] accent-accent cursor-pointer shrink-0"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="settings-agent-socket" class="text-xs font-semibold text-secondary">Custom Agent Socket Path</label>
          <span class="text-[11px] text-muted">Use a custom system socket path instead of the internal agent</span>
          <input
            id="settings-agent-socket"
            type="text"
            placeholder="e.g. /tmp/custom-agent.sock (blank to use default)"
            value={settings.custom_agent_socket}
            onchange={(e) => {
              settings.custom_agent_socket = (e.target as HTMLInputElement).value;
              onSave();
            }}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
          />
        </div>

        <div class="h-px bg-border/50 my-1"></div>

        <div class="flex flex-col gap-4">
          <h4 class="text-[10px] font-bold tracking-widest text-muted uppercase">Fallback Connection Values</h4>

          <div class="grid grid-cols-2 gap-4">
            <div class="flex flex-col gap-1.5">
              <label for="settings-default-user" class="text-xs font-medium text-secondary">Default Username</label>
              <input
                id="settings-default-user"
                type="text"
                value={settings.default_user}
                onchange={(e) => {
                  settings.default_user = (e.target as HTMLInputElement).value;
                  handleDefaultsSave();
                }}
                class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
              />
            </div>

            <div class="flex flex-col gap-1.5">
              <label for="settings-default-port" class="text-xs font-medium text-secondary">Default Port</label>
              <input
                id="settings-default-port"
                type="number"
                value={settings.default_port}
                onchange={(e) => {
                  settings.default_port = Number((e.target as HTMLInputElement).value);
                  handleDefaultsSave();
                }}
                class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
              />
            </div>
          </div>
        </div>
      </div>
    {/if}

    {#if activeCategory === "kerberos"}
      <div class="flex flex-col gap-6 max-w-2xl">
        <div>
          <h3 class="text-base font-semibold text-primary m-0">Kerberos GSSAPI</h3>
          <p class="text-xs text-muted mt-1">Configure Kerberos ticket expiry monitoring and automatic renewal warning thresholds</p>
        </div>

        <div class="h-px bg-border/50"></div>

        <div class="flex items-center justify-between gap-4 py-1">
          <div class="flex flex-col gap-0.5">
            <span class="text-xs font-semibold text-secondary">Monitor ticket expiry</span>
            <span class="text-[11px] text-muted leading-snug">Track remaining ticket lifetime and warn before credentials expire</span>
          </div>
          <input
            type="checkbox"
            checked={settings.monitor_kerberos}
            onchange={(e) => {
              settings.monitor_kerberos = (e.target as HTMLInputElement).checked;
              onSave();
            }}
            class="w-[18px] h-[18px] accent-accent cursor-pointer shrink-0"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="settings-kerberos-warn" class="text-xs font-semibold text-secondary">Warning Threshold (Minutes)</label>
          <span class="text-[11px] text-muted">Opens the renew ticket prompt when your ticket has less than this many minutes remaining</span>
          <input
            id="settings-kerberos-warn"
            type="number"
            min="1"
            max="1440"
            value={settings.kerberos_warn_minutes}
            onchange={(e) => {
              settings.kerberos_warn_minutes = Number((e.target as HTMLInputElement).value);
              onSave();
            }}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
          />
        </div>
      </div>
    {/if}

    {#if activeCategory === "logs"}
      <div class="flex flex-col gap-6 max-w-2xl">
        <div>
          <h3 class="text-base font-semibold text-primary m-0">Session Logs</h3>
          <p class="text-xs text-muted mt-1">Configure automatic history logs and application diagnostic logs</p>
        </div>

        <div class="h-px bg-border/50"></div>

        <div class="flex items-center justify-between gap-4 py-1">
          <div class="flex flex-col gap-0.5">
            <span class="text-xs font-semibold text-secondary">Record Session History</span>
            <span class="text-[11px] text-muted leading-snug">Save SSH session connection events to the workspace database</span>
          </div>
          <input
            type="checkbox"
            checked={workspace.auto_save_history}
            onchange={(e) => {
              workspace = {
                ...workspace,
                auto_save_history: (e.target as HTMLInputElement).checked,
              };
              handleWorkspaceSave();
            }}
            class="w-[18px] h-[18px] accent-accent cursor-pointer shrink-0"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="settings-max-history" class="text-xs font-semibold text-secondary">Maximum Log Entries</label>
          <span class="text-[11px] text-muted">Maximum count of historical session records kept in the database</span>
          <input
            id="settings-max-history"
            type="number"
            min="50"
            max="100000"
            step="50"
            bind:value={workspace.max_history_size}
            onchange={handleWorkspaceSave}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="settings-log-level" class="text-xs font-semibold text-secondary">Application Log Level</label>
          <span class="text-[11px] text-muted">Controls backend diagnostic log granularity</span>
          <select
            id="settings-log-level"
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] cursor-pointer transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
            bind:value={workspace.log_level}
            onchange={handleWorkspaceSave}
          >
            <option value="trace">Trace</option>
            <option value="debug">Debug</option>
            <option value="info">Info</option>
            <option value="warn">Warn</option>
            <option value="error">Error</option>
            <option value="off">Off</option>
          </select>
        </div>
      </div>
    {/if}

    {#if activeCategory === "appearance"}
      <div class="flex flex-col gap-6 max-w-2xl">
        <div>
          <h3 class="text-base font-semibold text-primary m-0">Appearance & Locale</h3>
          <p class="text-xs text-muted mt-1">Configure active UI color theme and application timezone</p>
        </div>

        <div class="h-px bg-border/50"></div>

        <div class="flex flex-col gap-1.5">
          <label for="settings-theme" class="text-xs font-semibold text-secondary">Active UI Theme</label>
          <span class="text-[11px] text-muted">Choose your preferred visual style and colors</span>
          <select
            id="settings-theme"
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] cursor-pointer transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
            value={settings.theme}
            onchange={(e) => onThemeChange((e.target as HTMLSelectElement).value)}
          >
            <option value="zinc">Slate Minimalist (Zinc)</option>
            <option value="cyberpunk">Cyberpunk Neon (Dark Glow)</option>
            <option value="oled">OLED Pitch Black</option>
            <option value="slate">Sleek Navy (Slate)</option>
          </select>
        </div>

        <div class="flex flex-col gap-2">
          <label for="settings-timezone" class="text-xs font-semibold text-secondary">Application Timezone</label>
          <span class="text-[11px] text-muted">Dates and times across logs and metrics are displayed in this timezone</span>
          <div class="flex flex-col gap-1.5 mt-1">
            <input
              id="settings-timezone-filter"
              type="text"
              placeholder="Filter timezones..."
              bind:value={timezoneFilter}
              class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
            />
            <select
              id="settings-timezone"
              class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] cursor-pointer transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
              value={settings.timezone}
              onchange={(e) => {
                settings.timezone = (e.target as HTMLSelectElement).value;
                onSave();
              }}
            >
              <option value={SYSTEM_TIMEZONE}>System default ({systemTimezone})</option>
              {#each filteredTimezoneOptions as tz}
                <option value={tz}>{tz}</option>
              {/each}
            </select>
          </div>
        </div>
      </div>
    {/if}
  </div>
</div>
