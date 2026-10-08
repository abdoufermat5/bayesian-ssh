<script lang="ts">
  import { Download, FolderOpen, RefreshCw, Upload } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { DesktopSettings, EnvInfo, WorkspaceInfo } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { handleDefaultsSave, handleWorkspaceSave } from "./helpers";

  interface Props {
    settings: DesktopSettings;
    workspace: WorkspaceInfo;
    environments: EnvInfo[];
    onSave: () => void;
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
    onSaveWorkspace,
    onSwitchEnv,
    onManageProfiles,
    onBrowseSshConfig,
    onImportSshConfig,
  }: Props = $props();

  let backupPassphrase = $state("");
  let sshConfigPath = $state(workspace.ssh_config_path || "");

  $effect(() => {
    sshConfigPath = workspace.ssh_config_path || "";
  });

  const profileOptions = $derived(environments.map((env) => ({ value: env.name, label: env.name })));

  const rankingOptions = [
    { value: "bayesian", label: "Usage-ranked", description: "Frequency + recency scoring" },
    { value: "fuzzy", label: "Fuzzy match", description: "Literal text matching" },
  ];

  const systemPaths = $derived([
    { label: "Config root", value: workspace.config_root },
    { label: "Profile directory", value: workspace.env_dir },
    { label: "Database", value: workspace.database_path },
  ]);

  function saveWorkspace() {
    workspace = handleWorkspaceSave(workspace, settings, sshConfigPath, onSaveWorkspace);
  }

  function saveDefaults() {
    workspace = handleDefaultsSave(workspace, settings, sshConfigPath, onSave, onSaveWorkspace);
  }

  async function handleExportEncryptedBackup() {
    try {
      const path = await invoke<string | null>("save_backup_file");
      if (!path) return;
      const msg = await invoke<string>("export_connections_payload", {
        outputPath: path,
        passphrase: backupPassphrase || null,
        format: "json",
        tag: null,
      });
      notify(msg, "success");
    } catch (err) {
      notify(`Export backup failed: ${err}`, "error");
    }
  }

  async function handleImportEncryptedBackup() {
    try {
      const path = await invoke<string | null>("pick_backup_file");
      if (!path) return;
      const count = await invoke<number>("import_connections_payload", {
        filePath: path,
        passphrase: backupPassphrase || null,
        noBastion: false,
      });
      notify(`Imported ${count} connection(s)`, "success");
    } catch (err) {
      notify(`Import backup failed: ${err}`, "error");
    }
  }
</script>

<div class="settings-page">
  <div>
    <h2 class="settings-heading">Profiles & workspace</h2>
    <p class="settings-desc">Environment profiles, host search and where your data lives.</p>
  </div>

  <section>
    <h3 class="settings-group-title">Profile</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Active profile</span>
          <span class="setting-meta">Hosts and credentials are isolated per profile.</span>
        </div>
        <div class="setting-control">
          <CustomSelect
            id="settings-profile"
            class="w-48"
            size="md"
            options={profileOptions}
            value={workspace.active_env}
            onChange={(val) => onSwitchEnv(val)}
          />
          <button type="button" class="btn btn-secondary" onclick={onManageProfiles}>Manage</button>
        </div>
      </div>
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Host ranking</span>
          <span class="setting-meta">How hosts are ordered when you search.</span>
        </div>
        <div class="setting-control">
          <CustomSelect
            class="w-48"
            size="md"
            options={rankingOptions}
            value={settings.fuzzy_search ? "fuzzy" : "bayesian"}
            onChange={(val) => {
              settings.fuzzy_search = val === "fuzzy";
              saveDefaults();
            }}
          />
        </div>
      </div>
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">OpenSSH config</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="settings-ssh-config">Config file</label>
          <span class="setting-meta">Used when importing hosts.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-ssh-config"
            type="text"
            placeholder="~/.ssh/config"
            bind:value={sshConfigPath}
            onchange={saveWorkspace}
            class="input input-mono w-56"
          />
          <button type="button" class="btn btn-secondary" onclick={onBrowseSshConfig}>
            <FolderOpen size={14} />
            Browse
          </button>
        </div>
      </div>
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Import hosts</span>
          <span class="setting-meta">Add hosts defined in the config file to this profile.</span>
        </div>
        <div class="setting-control">
          <button type="button" class="btn btn-secondary" onclick={onImportSshConfig}>
            <RefreshCw size={14} />
            Import hosts
          </button>
        </div>
      </div>
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">Backup & restore</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="settings-backup-passphrase">Passphrase</label>
          <span class="setting-meta">Encrypts the backup with AES-256-GCM. Leave empty for plain JSON.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-backup-passphrase"
            type="password"
            placeholder="Optional"
            autocomplete="new-password"
            bind:value={backupPassphrase}
            class="input w-48"
          />
        </div>
      </div>
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Hosts backup</span>
          <span class="setting-meta">Export every host in this profile, or restore from a backup file.</span>
        </div>
        <div class="setting-control">
          <button type="button" class="btn btn-secondary" onclick={handleImportEncryptedBackup}>
            <Upload size={14} />
            Restore…
          </button>
          <button type="button" class="btn btn-secondary" onclick={handleExportEncryptedBackup}>
            <Download size={14} />
            Export…
          </button>
        </div>
      </div>
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">Storage</h3>
    <div class="settings-group">
      {#each systemPaths as p (p.label)}
        <div class="setting-row">
          <span class="setting-title shrink-0">{p.label}</span>
          <code class="system-value min-w-0 max-w-[70%]" title={p.value}>{p.value}</code>
        </div>
      {/each}
    </div>
  </section>
</div>
