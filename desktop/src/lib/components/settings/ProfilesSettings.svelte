<script lang="ts">
  import {
    ShieldCheck,
    FolderPlus,
    FolderOpen,
    RefreshCw,
    Database,
    Download,
    Upload,
  } from "lucide-svelte";
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
      notify(`Successfully imported ${count} connection(s)!`, "success");
    } catch (err) {
      notify(`Import backup failed: ${err}`, "error");
    }
  }
</script>

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
      <CustomSelect
        id="settings-profile"
        class="flex-1"
        options={environments.map((env) => ({ value: env.name, label: env.name }))}
        value={workspace.active_env}
        onChange={(val) => onSwitchEnv(val)}
      />
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
        bind:value={sshConfigPath}
        onchange={saveWorkspace}
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
        Import OpenSSH hosts
      </button>
    </div>
  </div>

  <!-- Encrypted Backup & Backup Export Section -->
  <div class="h-px bg-border/50 my-1"></div>
  <div class="flex flex-col gap-2">
    <h4 class="text-[10px] font-bold tracking-widest text-muted uppercase flex items-center gap-2">
      <ShieldCheck size={12} class="text-accent" />
      <span>Encrypted Backup & Restore</span>
    </h4>
    <p class="text-[11px] text-muted">Export your server database encrypted with AES-256-GCM / PBKDF2 or restore from an encrypted backup.</p>
    <div class="flex flex-wrap gap-2 mt-1">
      <input
        type="password"
        placeholder="Passphrase (optional)"
        bind:value={backupPassphrase}
        class="bg-surface-input border border-border text-primary py-1.5 px-3 rounded-lg outline-none text-xs w-[220px]"
      />
      <button
        type="button"
        class="bg-accent/15 border border-accent/30 text-accent py-1.5 px-3 rounded-lg cursor-pointer font-semibold flex items-center gap-1.5 text-xs hover:bg-accent hover:text-white transition-all"
        onclick={handleExportEncryptedBackup}
      >
        <Download size={13} />
        Export Encrypted Backup
      </button>
      <button
        type="button"
        class="bg-white/[0.04] border border-border text-secondary py-1.5 px-3 rounded-lg cursor-pointer font-semibold flex items-center gap-1.5 text-xs hover:text-primary hover:bg-white/[0.06] transition-all"
        onclick={handleImportEncryptedBackup}
      >
        <Upload size={13} />
        Import Backup File
      </button>
    </div>
  </div>

  <div class="flex items-center justify-between gap-4 py-2">
    <div class="flex flex-col gap-0.5">
      <span class="text-xs font-semibold text-secondary">Host Ranking Mode</span>
      <span class="text-[11px] text-muted">Bayesian uses frequency + recency; fuzzy uses text matching</span>
    </div>
    <CustomSelect
      options={[
        { value: "bayesian", label: "Bayesian ranking", description: "Frequency + recency scoring" },
        { value: "fuzzy", label: "Fuzzy search", description: "Literal text matching" }
      ]}
      value={settings.fuzzy_search ? "fuzzy" : "bayesian"}
      onChange={(val) => {
        settings.fuzzy_search = val === "fuzzy";
        saveDefaults();
      }}
    />
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
