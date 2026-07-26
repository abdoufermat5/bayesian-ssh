<script lang="ts">
  import { Terminal, Upload, ArrowRight, Lock, KeyRound } from "lucide-svelte";
  import type { OnboardingPayload } from "$lib/types";
  import { invoke } from "@tauri-apps/api/core";
  import { notify } from "$lib/stores/notifications.svelte";

  interface Props {
    defaultUser: string;
    defaultSshConfigPath: string;
    configRoot: string;
    onBrowseSshConfig: () => Promise<string | null>;
    onComplete: (payload: OnboardingPayload) => Promise<void>;
  }

  let { defaultUser, defaultSshConfigPath, configRoot, onBrowseSshConfig, onComplete }: Props = $props();

  let busy = $state(false);
  let showAdvanced = $state(false);

  // Form fields
  let default_user = $state("");
  let default_port = $state(22);
  let ssh_config_path = $state("");
  let profileName = $state("default");
  let import_ssh_config = $state(true);

  // Backup restore state
  let selectedBackupPath = $state("");
  let restorePassphrase = $state("");
  let showPassphraseInput = $state(false);

  $effect(() => {
    if (defaultUser && !default_user) {
      default_user = defaultUser;
    }
    if (defaultSshConfigPath && !ssh_config_path) {
      ssh_config_path = defaultSshConfigPath;
    }
  });

  async function handleQuickStart() {
    busy = true;
    try {
      await onComplete({
        profile_name: profileName.trim() || "default",
        create_profile: profileName.trim() !== "default" && profileName.trim().length > 0,
        default_user: default_user.trim() || defaultUser || "root",
        default_port: default_port || 22,
        ssh_config_path: ssh_config_path.trim() || null,
        theme: "zinc",
        auto_start_agent: false,
        import_ssh_config,
        fuzzy_search: false,
      });
    } finally {
      busy = false;
    }
  }

  async function handleRestoreBackup() {
    try {
      const path = await invoke<string | null>("pick_backup_file");
      if (!path) return;
      selectedBackupPath = path;
      await executeRestore(path, null);
    } catch (err) {
      notify(`Restore failed: ${err}`, "error");
    }
  }

  async function executeRestore(path: string, pass: string | null) {
    busy = true;
    try {
      const count = await invoke<number>("import_connections_payload", {
        filePath: path,
        passphrase: pass,
        noBastion: false,
      });
      showPassphraseInput = false;
      await onComplete({
        profile_name: "default",
        create_profile: false,
        default_user: default_user || defaultUser || "root",
        default_port: 22,
        ssh_config_path: null,
        theme: "zinc",
        auto_start_agent: false,
        import_ssh_config: false,
        fuzzy_search: false,
      });
      notify(`Backup restored (${count} connection(s) loaded)`, "success");
    } catch (err: unknown) {
      const errStr = String(err);
      if (errStr.includes("requires passphrase") || errStr.includes("decrypt") || errStr.includes("Passphrase")) {
        showPassphraseInput = true;
      } else {
        notify(`Restore failed: ${err}`, "error");
      }
    } finally {
      busy = false;
    }
  }

  async function submitPassphrase() {
    if (!restorePassphrase.trim() || !selectedBackupPath) return;
    await executeRestore(selectedBackupPath, restorePassphrase.trim());
  }
</script>

<div class="fixed inset-0 bg-[#09090b] z-[200] flex flex-col items-center justify-center p-6 select-none overflow-y-auto">
  <div class="w-full max-w-[480px] flex flex-col gap-6 my-auto">
    
    <!-- Header -->
    <div class="flex flex-col items-center text-center gap-2">
      <div class="w-12 h-12 rounded-2xl bg-white/[0.05] border border-white/10 flex items-center justify-center mb-1 text-primary shadow-inner">
        <Terminal size={24} />
      </div>
      <h1 class="m-0 text-xl font-bold tracking-tight text-white">Bayesian SSH</h1>
      <p class="m-0 text-xs text-muted max-w-[340px]">Fast, intelligent SSH session manager with Bayesian search.</p>
    </div>

    <!-- Primary Action Cards -->
    <div class="flex flex-col gap-3">
      
      <!-- Option 1: Quick Start -->
      <button
        type="button"
        class="group p-4 rounded-xl border border-white/10 bg-white/[0.02] hover:bg-white/[0.05] hover:border-white/20 transition-all duration-150 flex items-center justify-between text-left cursor-pointer disabled:opacity-50"
        onclick={handleQuickStart}
        disabled={busy}
      >
        <div class="flex items-center gap-3.5">
          <div class="w-9 h-9 rounded-lg bg-accent/10 border border-accent/20 text-accent flex items-center justify-center shrink-0">
            <ArrowRight size={18} />
          </div>
          <div>
            <div class="text-xs font-semibold text-white group-hover:text-accent transition-colors">Start Fresh Workspace</div>
            <div class="text-[11px] text-muted mt-0.5">Initialize default profile & import OpenSSH config</div>
          </div>
        </div>
        <span class="text-xs font-medium text-muted group-hover:text-white transition-colors">Setup &rarr;</span>
      </button>

      <!-- Option 2: Restore Backup -->
      <button
        type="button"
        class="group p-4 rounded-xl border border-white/10 bg-white/[0.02] hover:bg-white/[0.05] hover:border-white/20 transition-all duration-150 flex items-center justify-between text-left cursor-pointer disabled:opacity-50"
        onclick={handleRestoreBackup}
        disabled={busy}
      >
        <div class="flex items-center gap-3.5">
          <div class="w-9 h-9 rounded-lg bg-white/[0.05] border border-white/10 text-primary flex items-center justify-center shrink-0">
            <Upload size={18} />
          </div>
          <div>
            <div class="text-xs font-semibold text-white group-hover:text-accent transition-colors">Restore from Backup</div>
            <div class="text-[11px] text-muted mt-0.5">Load complete backup file (.json or .enc)</div>
          </div>
        </div>
        <span class="text-xs font-medium text-muted group-hover:text-white transition-colors">Open file &rarr;</span>
      </button>

    </div>

    <!-- Toggle Advanced Settings -->
    <div class="flex flex-col items-center">
      <button
        type="button"
        class="text-[11px] text-muted hover:text-secondary transition-colors cursor-pointer bg-transparent border-none py-1"
        onclick={() => (showAdvanced = !showAdvanced)}
      >
        {showAdvanced ? "Hide options" : "Advanced setup options \u2193"}
      </button>

      {#if showAdvanced}
        <div class="w-full mt-3 p-4 rounded-xl border border-white/10 bg-white/[0.01] flex flex-col gap-3 text-left">
          <div class="grid grid-cols-2 gap-3">
            <div class="flex flex-col gap-1">
              <label for="ob-user" class="text-[10px] font-semibold text-muted uppercase tracking-wider">Default User</label>
              <input
                id="ob-user"
                type="text"
                class="bg-white/[0.04] border border-white/10 text-white text-xs py-1.5 px-2.5 rounded-lg outline-none focus:border-accent"
                bind:value={default_user}
              />
            </div>
            <div class="flex flex-col gap-1">
              <label for="ob-port" class="text-[10px] font-semibold text-muted uppercase tracking-wider">Default Port</label>
              <input
                id="ob-port"
                type="number"
                class="bg-white/[0.04] border border-white/10 text-white text-xs py-1.5 px-2.5 rounded-lg outline-none focus:border-accent"
                bind:value={default_port}
              />
            </div>
          </div>

          <div class="flex flex-col gap-1">
            <label for="ob-profile" class="text-[10px] font-semibold text-muted uppercase tracking-wider">Workspace Name</label>
            <input
              id="ob-profile"
              type="text"
              class="bg-white/[0.04] border border-white/10 text-white text-xs py-1.5 px-2.5 rounded-lg outline-none focus:border-accent"
              placeholder="default"
              bind:value={profileName}
            />
          </div>

          <label class="flex items-center gap-2 text-xs text-secondary cursor-pointer mt-1">
            <input type="checkbox" bind:checked={import_ssh_config} class="accent-accent" />
            <span>Import ~/.ssh/config hosts automatically</span>
          </label>
        </div>
      {/if}
    </div>

    <!-- Footer note -->
    <div class="text-center text-[11px] text-muted/60 font-mono">
      Config path: {configRoot}
    </div>

  </div>
</div>

{#if showPassphraseInput}
  <div class="fixed inset-0 bg-black/90 backdrop-blur-md flex items-center justify-center z-[250] p-6">
    <div class="w-[380px] max-w-full bg-[#121215] border border-white/10 rounded-2xl p-5 flex flex-col gap-4 shadow-2xl">
      <div class="flex items-center gap-3 text-white">
        <Lock size={20} class="text-accent" />
        <div>
          <h3 class="m-0 text-sm font-bold">Encrypted Backup</h3>
          <p class="m-0 text-[11px] text-muted mt-0.5">Enter passphrase to decrypt and restore</p>
        </div>
      </div>

      <div class="flex flex-col gap-1">
        <input
          type="password"
          class="bg-white/[0.05] border border-white/10 text-white py-2 px-3 rounded-lg outline-none text-xs focus:border-accent"
          placeholder="Passphrase"
          bind:value={restorePassphrase}
          onkeydown={(e) => e.key === "Enter" && submitPassphrase()}
        />
      </div>

      <div class="flex justify-end gap-2">
        <button
          type="button"
          class="py-1.5 px-3 rounded-lg text-xs text-secondary hover:text-white bg-transparent border border-white/10 cursor-pointer"
          onclick={() => (showPassphraseInput = false)}
        >
          Cancel
        </button>
        <button
          type="button"
          class="py-1.5 px-3.5 rounded-lg text-xs font-semibold bg-accent text-white hover:bg-accent-hover flex items-center gap-1.5 cursor-pointer disabled:opacity-50"
          onclick={submitPassphrase}
          disabled={busy || !restorePassphrase.trim()}
        >
          <KeyRound size={13} />
          Decrypt
        </button>
      </div>
    </div>
  </div>
{/if}
