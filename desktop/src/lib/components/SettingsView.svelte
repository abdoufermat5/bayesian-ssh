<script lang="ts">
  import { FolderOpen, FolderPlus, RefreshCw } from "lucide-svelte";
  import type { DesktopSettings, EnvInfo, WorkspaceInfo } from "$lib/types";

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

  let sshConfigPath = $state(workspace.ssh_config_path || "");

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

<div class="page-view settings-view">
  <div class="page-view-header view-header settings-view-header">
    <div class="title-meta">
      <h2>Desktop Settings</h2>
      <span class="subtitle">Workspace, preferences, and SSH integration</span>
    </div>
  </div>

  <div class="page-view-scroll">
    <div class="settings-grid">
      <div class="settings-card settings-card-wide">
        <span class="settings-card-title">WORKSPACE</span>

        <div class="settings-field">
          <label for="settings-profile">Active profile</label>
          <div class="settings-inline-actions">
            <select
              id="settings-profile"
              class="cyber-select"
              value={workspace.active_env}
              onchange={(e) => onSwitchEnv((e.target as HTMLSelectElement).value)}
            >
              {#each environments as env}
                <option value={env.name}>{env.name}</option>
              {/each}
            </select>
            <button type="button" class="browse-btn" onclick={onManageProfiles}>
              <FolderPlus size={14} />
              Manage
            </button>
          </div>
        </div>

        <div class="settings-field">
          <span class="label-style">Config root</span>
          <code class="path-display">{workspace.config_root}</code>
        </div>

        <div class="settings-field">
          <span class="label-style">Profile directory</span>
          <code class="path-display">{workspace.env_dir}</code>
        </div>

        <div class="settings-field">
          <span class="label-style">Database</span>
          <code class="path-display">{workspace.database_path}</code>
        </div>

        <div class="settings-field">
          <label for="settings-ssh-config">OpenSSH config path</label>
          <div class="input-with-action">
            <input
              id="settings-ssh-config"
              type="text"
              placeholder="~/.ssh/config"
              bind:value={sshConfigPath}
              onchange={handleWorkspaceSave}
              class="cyber-input"
            />
            <button type="button" class="browse-btn" onclick={onBrowseSshConfig}>
              <FolderOpen size={14} />
              Browse
            </button>
          </div>
          <span class="settings-field-hint">Used for host import and connection defaults</span>
        </div>

        <div class="settings-row">
          <div class="settings-field">
            <span class="label-style">Host ranking mode</span>
            <span class="settings-field-hint">Bayesian uses frequency + recency; fuzzy uses text match</span>
          </div>
          <select
            class="cyber-select settings-inline-select"
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

        <div class="settings-actions">
          <button type="button" class="browse-btn" onclick={onImportSshConfig}>
            <RefreshCw size={14} />
            Import from OpenSSH config
          </button>
        </div>
      </div>

      <div class="settings-card">
        <span class="settings-card-title">APPEARANCE</span>

        <div class="settings-field">
          <label for="settings-theme">Active Theme</label>
          <select
            id="settings-theme"
            class="cyber-select"
            value={settings.theme}
            onchange={(e) => onThemeChange((e.target as HTMLSelectElement).value)}
          >
            <option value="zinc">Slate Minimalist (Zinc)</option>
            <option value="cyberpunk">Cyberpunk Neon (Dark Glow)</option>
            <option value="oled">OLED Pitch Black</option>
            <option value="slate">Sleek Navy (Slate)</option>
          </select>
        </div>
      </div>

      <div class="settings-card">
        <span class="settings-card-title">SSH AGENT INTEGRATION</span>

        <div class="settings-row">
          <div class="settings-field">
            <span class="label-style">Auto-start SSH Agent</span>
            <span class="settings-field-hint">Start agent automatically on desktop app startup</span>
          </div>
          <input
            type="checkbox"
            bind:checked={settings.auto_start_agent}
            onchange={onSave}
            class="settings-checkbox"
          />
        </div>

        <div class="settings-field">
          <label for="settings-agent-socket">Custom Agent Socket Path</label>
          <input
            id="settings-agent-socket"
            type="text"
            placeholder="e.g. /tmp/custom-agent.sock (blank to use default)"
            bind:value={settings.custom_agent_socket}
            onchange={onSave}
            class="cyber-input"
          />
        </div>
      </div>

      <div class="settings-card">
        <span class="settings-card-title">CONNECTION DEFAULTS</span>

        <div class="settings-field">
          <label for="settings-default-user">Default SSH User</label>
          <input
            id="settings-default-user"
            type="text"
            bind:value={settings.default_user}
            onchange={handleDefaultsSave}
            class="cyber-input"
          />
        </div>

        <div class="settings-field">
          <label for="settings-default-port">Default SSH Port</label>
          <input
            id="settings-default-port"
            type="number"
            bind:value={settings.default_port}
            onchange={handleDefaultsSave}
            class="cyber-input"
          />
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .settings-view-header {
    border-bottom: 1px solid var(--border-color);
    padding-bottom: 1rem;
  }

  .label-style {
    font-size: 12px;
    font-weight: 500;
  }

  .settings-checkbox {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-cyan);
    flex-shrink: 0;
  }

  .settings-card-wide {
    grid-column: 1 / -1;
  }

  .settings-inline-actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .settings-inline-actions .cyber-select {
    flex: 1;
  }

  .settings-inline-select {
    min-width: 180px;
  }

  .path-display {
    display: block;
    padding: 10px 12px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: rgba(255, 255, 255, 0.02);
    color: var(--text-secondary);
    font-family: "JetBrains Mono", monospace;
    font-size: 0.75rem;
    word-break: break-all;
  }

  .settings-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 4px;
  }
</style>
