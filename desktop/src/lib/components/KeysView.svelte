<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { KeyRound, Plus, Send, ShieldCheck, ShieldAlert, RefreshCw, Copy, Check } from "lucide-svelte";
  import type { SshKeyInfo, Connection } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { notify } from "$lib/stores/notifications.svelte";

  interface Props {
    connections: Connection[];
  }

  let { connections }: Props = $props();

  let keys = $state<SshKeyInfo[]>([]);
  let loading = $state(true);
  let copiedFingerprint = $state<string | null>(null);

  // Generate Key Modal
  let showGenerateModal = $state(false);
  let genKeyName = $state("id_ed25519_bssh");
  let genKeyType = $state("ed25519");
  let generating = $state(false);

  // Copy Key Modal
  let showCopyModal = $state(false);
  let selectedKeyPath = $state<string>("");
  let selectedTarget = $state<string>("");
  let copying = $state(false);

  async function loadKeys() {
    loading = true;
    try {
      keys = await invoke<SshKeyInfo[]>("list_ssh_keys");
    } catch (err) {
      notify(`Failed to load SSH keys: ${err}`, "error");
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadKeys();
  });

  async function handleGenerateKey() {
    if (!genKeyName.trim()) {
      notify("Key name is required", "error");
      return;
    }
    generating = true;
    try {
      await invoke("generate_ssh_key", {
        name: genKeyName.trim(),
        keyType: genKeyType,
      });
      notify(`SSH Key '${genKeyName}' generated successfully!`, "success");
      showGenerateModal = false;
      genKeyName = "id_ed25519_bssh";
      await loadKeys();
    } catch (err) {
      notify(`Key generation failed: ${err}`, "error");
    } finally {
      generating = false;
    }
  }

  async function handleCopyKey() {
    if (!selectedTarget) {
      notify("Please select a target host", "error");
      return;
    }
    copying = true;
    try {
      const msg = await invoke<string>("copy_ssh_key_to_target", {
        target: selectedTarget,
        keyPath: selectedKeyPath || null,
      });
      notify(msg, "success");
      showCopyModal = false;
    } catch (err) {
      notify(`Copying key failed: ${err}`, "error");
    } finally {
      copying = false;
    }
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    copiedFingerprint = text;
    setTimeout(() => (copiedFingerprint = null), 2000);
  }
</script>

<div class="flex flex-col flex-1 h-full min-h-0 overflow-y-auto p-6 bg-surface text-primary">
  <!-- Header -->
  <div class="flex items-center justify-between mb-6 pb-4 border-b border-border">
    <div>
      <h1 class="text-xl font-bold tracking-tight flex items-center gap-2 text-primary">
        <KeyRound class="text-accent" size={24} />
        SSH Key Manager
      </h1>
      <p class="text-xs text-muted mt-1">
        Manage local identity keys in <code class="bg-surface-input px-1.5 py-0.5 rounded text-accent font-mono">~/.ssh/</code>, verify permissions, and deploy public keys to remote servers.
      </p>
    </div>
    <div class="flex items-center gap-2">
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-border bg-surface-input text-xs font-medium cursor-pointer transition-all hover:bg-white/5 hover:border-border-hover text-primary"
        onclick={loadKeys}
        disabled={loading}
      >
        <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
        Refresh
      </button>
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-medium cursor-pointer transition-all hover:opacity-90 shadow-sm"
        onclick={() => (showGenerateModal = true)}
      >
        <Plus size={14} />
        Generate Key
      </button>
    </div>
  </div>

  <!-- Key Inventory Grid -->
  {#if loading}
    <div class="flex flex-col items-center justify-center py-16 text-muted">
      <RefreshCw class="animate-spin mb-2" size={28} />
      <span class="text-xs">Scanning ~/.ssh/ directory...</span>
    </div>
  {:else if keys.length === 0}
    <div class="flex flex-col items-center justify-center py-16 text-muted border border-dashed border-border rounded-xl">
      <KeyRound size={40} class="mb-3 opacity-40 text-accent" />
      <span class="text-sm font-semibold text-primary mb-1">No SSH Keys Found</span>
      <span class="text-xs max-w-sm text-center mb-4">No public SSH keys (*.pub) were found in your ~/.ssh/ directory.</span>
      <button
        class="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-accent text-white text-xs font-semibold cursor-pointer shadow-sm hover:opacity-90"
        onclick={() => (showGenerateModal = true)}
      >
        <Plus size={14} />
        Generate Your First Key
      </button>
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      {#each keys as key}
        <div class="flex flex-col p-4 rounded-xl border border-border bg-surface-input/50 relative group transition-all duration-200 hover:border-border-hover hover:bg-surface-input/80">
          <div class="flex items-start justify-between mb-3">
            <div class="flex items-center gap-2">
              <span class="px-2 py-0.5 rounded text-[11px] font-bold uppercase tracking-wider bg-accent/15 text-accent border border-accent/20">
                {key.key_type}
              </span>
              <span class="font-bold text-sm text-primary">{key.name}</span>
            </div>
            {#if key.is_secure}
              <span class="flex items-center gap-1 text-[11px] font-semibold text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
                <ShieldCheck size={12} />
                {key.private_key_permission}
              </span>
            {:else}
              <span class="flex items-center gap-1 text-[11px] font-semibold text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded-full border border-amber-500/20">
                <ShieldAlert size={12} />
                {key.private_key_permission}
              </span>
            {/if}
          </div>

          <!-- Fingerprint -->
          <div class="flex items-center justify-between bg-black/20 p-2 rounded-lg border border-white/5 mb-3 font-mono text-[11px] text-muted">
            <span class="truncate mr-2">{key.fingerprint}</span>
            <button
              class="text-muted hover:text-primary transition-colors p-1"
              onclick={() => copyToClipboard(key.fingerprint)}
              title="Copy Fingerprint"
            >
              {#if copiedFingerprint === key.fingerprint}
                <Check size={13} class="text-emerald-400" />
              {:else}
                <Copy size={13} />
              {/if}
            </button>
          </div>

          <!-- Paths & Comment -->
          <div class="space-y-1 text-xs text-muted mb-4">
            <div class="truncate">
              <span class="font-semibold text-primary/70">Public Key:</span> <span class="font-mono text-[11px] text-muted">{key.public_key_path}</span>
            </div>
            {#if key.private_key_path}
              <div class="truncate">
                <span class="font-semibold text-primary/70">Private Key:</span> <span class="font-mono text-[11px] text-muted">{key.private_key_path}</span>
              </div>
            {/if}
            {#if key.comment}
              <div class="truncate">
                <span class="font-semibold text-primary/70">Comment:</span> {key.comment}
              </div>
            {/if}
          </div>

          <!-- Action Button -->
          <div class="mt-auto pt-3 border-t border-white/5 flex justify-end">
            <button
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-border bg-surface text-xs font-medium text-primary hover:bg-white/5 hover:border-border-hover transition-all"
              onclick={() => {
                selectedKeyPath = key.public_key_path;
                selectedTarget = connections[0]?.name || "";
                showCopyModal = true;
              }}
            >
              <Send size={13} class="text-accent" />
              Deploy to Server
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Generate Key Modal -->
  {#if showGenerateModal}
    <ModalShell
      open={showGenerateModal}
      title="Generate New SSH Keypair"
      onClose={() => (showGenerateModal = false)}
      width="md"
      panelClass="p-6 space-y-4"
    >
      <h2 class="text-base font-bold text-primary flex items-center gap-2">
        <KeyRound class="text-accent" size={20} />
        Generate New SSH Keypair
      </h2>

        <div>
          <label for="gen-key-name" class="block text-xs font-semibold text-muted mb-1">Key Filename (~/.ssh/)</label>
          <input
            id="gen-key-name"
            type="text"
            class="w-full bg-surface-input border border-border rounded-lg px-3 py-2 text-xs text-primary outline-none focus:border-accent"
            bind:value={genKeyName}
            placeholder="e.g. id_ed25519_prod"
          />
        </div>

        <div>
          <label id="gen-key-type-label" for="gen-key-type" class="block text-xs font-semibold text-muted mb-1">Key Algorithm</label>
          <CustomSelect
            id="gen-key-type"
            options={[
              { value: "ed25519", label: "Ed25519", description: "Recommended - High security & fast" },
              { value: "rsa", label: "RSA 4096-bit", description: "Legacy compatibility" }
            ]}
            bind:value={genKeyType}
          />
        </div>

        <div class="flex justify-end gap-2 pt-2">
          <button
            class="px-3.5 py-1.5 rounded-lg border border-border text-xs text-muted hover:text-primary hover:bg-white/5"
            onclick={() => (showGenerateModal = false)}
            disabled={generating}
          >
            Cancel
          </button>
          <button
            class="px-4 py-1.5 rounded-lg bg-accent text-white text-xs font-semibold hover:opacity-90 flex items-center gap-1.5"
            onclick={handleGenerateKey}
            disabled={generating}
          >
            {#if generating}
              <RefreshCw size={14} class="animate-spin" />
              Generating...
            {:else}
              Generate
            {/if}
          </button>
        </div>
    </ModalShell>
  {/if}

  <!-- Copy Key Modal -->
  {#if showCopyModal}
    <ModalShell
      open={showCopyModal}
      title="Deploy Public Key to Target"
      onClose={() => (showCopyModal = false)}
      width="md"
      panelClass="p-6 space-y-4"
    >
      <h2 class="text-base font-bold text-primary flex items-center gap-2">
        <Send class="text-accent" size={20} />
        Deploy Public Key to Target
      </h2>

        <div>
          <label for="copy-key-path" class="block text-xs font-semibold text-muted mb-1">Public Key Path</label>
          <input
            id="copy-key-path"
            type="text"
            class="w-full bg-surface-input border border-border rounded-lg px-3 py-2 text-xs text-muted font-mono"
            readonly
            value={selectedKeyPath}
          />
        </div>

        <div>
          <label id="copy-key-target-label" for="copy-key-target" class="block text-xs font-semibold text-muted mb-1">Target Host Connection</label>
          <CustomSelect
            id="copy-key-target"
            placeholder="Select target host connection..."
            options={connections.map((c) => ({ value: c.name, label: c.name, description: `${c.user}@${c.host}:${c.port}` }))}
            bind:value={selectedTarget}
          />
        </div>

        <div class="flex justify-end gap-2 pt-2">
          <button
            class="px-3.5 py-1.5 rounded-lg border border-border text-xs text-muted hover:text-primary hover:bg-white/5"
            onclick={() => (showCopyModal = false)}
            disabled={copying}
          >
            Cancel
          </button>
          <button
            class="px-4 py-1.5 rounded-lg bg-accent text-white text-xs font-semibold hover:opacity-90 flex items-center gap-1.5"
            onclick={handleCopyKey}
            disabled={copying || !selectedTarget}
          >
            {#if copying}
              <RefreshCw size={14} class="animate-spin" />
              Deploying...
            {:else}
              Deploy Public Key
            {/if}
          </button>
        </div>
    </ModalShell>
  {/if}
</div>
