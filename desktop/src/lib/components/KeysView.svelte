<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import {
    Check,
    Copy,
    KeyRound,
    Plus,
    RefreshCw,
    Send,
    ShieldAlert,
    ShieldCheck,
    X,
  } from "lucide-svelte";
  import type { Connection, SshKeyInfo } from "$lib/types";
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

  // Generate Key Modal State
  let showGenerateModal = $state(false);
  let genKeyName = $state("id_ed25519_bssh");
  let genKeyType = $state("ed25519");
  let generating = $state(false);

  // Copy Key to Target Host Modal State
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
    notify("Copied fingerprint to clipboard", "success");
    setTimeout(() => {
      if (copiedFingerprint === text) copiedFingerprint = null;
    }, 2000);
  }
</script>

<div class="flex flex-col flex-1 h-full min-h-0 overflow-y-auto px-6 py-5 bg-surface text-primary select-none scrollbar-none">
  <!-- Header -->
  <div class="flex items-center justify-between gap-4 pb-4 border-b border-border shrink-0 flex-wrap">
    <div class="flex items-center gap-3">
      <div class="w-8 h-8 rounded-xl bg-accent/15 border border-accent/30 flex items-center justify-center text-accent shrink-0">
        <KeyRound size={18} />
      </div>
      <div>
        <h2 class="text-sm font-bold text-primary tracking-tight m-0 flex items-center gap-2">
          SSH Keychain &amp; Identities
          <span class="badge-pill bg-accent/15 text-accent border border-accent/30 text-[10px]">
            {keys.length} keys
          </span>
        </h2>
        <p class="text-[11px] text-muted m-0">Cryptographic key pairs, fingerprints, and remote deployment</p>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={loadKeys}
        disabled={loading}
        title="Refresh key list"
      >
        <RefreshCw size={13} class={loading ? "animate-spin" : ""} />
        <span>Refresh</span>
      </button>

      <button
        type="button"
        class="btn btn-primary shadow-sm"
        onclick={() => (showGenerateModal = true)}
      >
        <Plus size={14} />
        <span>Generate Key</span>
      </button>
    </div>
  </div>

  <!-- Key List -->
  <div class="py-4 space-y-3 flex-1">
    {#if loading}
      <div class="py-24 flex flex-col items-center justify-center text-muted gap-2">
        <RefreshCw size={28} class="text-accent animate-spin" />
        <span class="text-xs font-semibold text-primary">Scanning ~/.ssh identities...</span>
      </div>
    {:else if keys.length > 0}
      <div class="grid grid-cols-1 md:grid-cols-2 gap-3.5">
        {#each keys as key}
          <div class="rounded-xl border border-border bg-surface-input/30 p-4 transition-all hover:border-border-hover hover:bg-surface-input/50 flex flex-col justify-between">
            <div>
              <!-- Key Card Header -->
              <div class="flex items-center justify-between gap-2 mb-2">
                <div class="flex items-center gap-2 min-w-0">
                  <KeyRound size={15} class="text-accent shrink-0" />
                  <span class="font-bold text-sm text-primary truncate">{key.name}</span>
                </div>
                <div class="flex items-center gap-1.5 shrink-0">
                  <span class="badge-pill bg-accent/15 border border-accent/30 text-accent font-mono text-[9px] uppercase">
                    {key.key_type}
                  </span>
                  {#if key.is_secure}
                    <span class="badge-pill bg-running/15 text-running border border-running/30 text-[9px]">
                      <ShieldCheck size={10} /> 0600
                    </span>
                  {:else}
                    <span class="badge-pill bg-error/15 text-error border border-error/30 text-[9px]">
                      <ShieldAlert size={10} /> {key.private_key_permission}
                    </span>
                  {/if}
                </div>
              </div>

              <!-- Fingerprint row -->
              <div class="p-2 rounded-lg bg-surface border border-border/80 flex items-center justify-between gap-2 font-mono text-[10px] text-muted mb-3">
                <span class="truncate">{key.fingerprint}</span>
                <button
                  type="button"
                  class="p-0.5 rounded text-muted hover:text-primary hover:bg-surface-hover border-none bg-transparent cursor-pointer shrink-0"
                  onclick={() => copyToClipboard(key.fingerprint)}
                  title="Copy SHA256 Fingerprint"
                >
                  {#if copiedFingerprint === key.fingerprint}
                    <Check size={12} class="text-running" />
                  {:else}
                    <Copy size={12} />
                  {/if}
                </button>
              </div>

              <!-- File Paths -->
              <div class="space-y-1 text-[11px] text-muted font-mono mb-3">
                <div class="truncate">
                  <span class="text-[9px] uppercase font-sans font-bold text-muted/70">Public: </span>
                  {key.public_key_path}
                </div>
                {#if key.private_key_path}
                  <div class="truncate">
                    <span class="text-[9px] uppercase font-sans font-bold text-muted/70">Private: </span>
                    {key.private_key_path}
                  </div>
                {/if}
              </div>
            </div>

            <!-- Footer Action Buttons -->
            <div class="flex items-center justify-between pt-2.5 border-t border-border/60">
              <span class="text-[10px] text-muted truncate max-w-[150px]">
                {key.comment || "No comment"}
              </span>

              <button
                type="button"
                class="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-accent/15 hover:bg-accent hover:text-white text-accent text-xs font-semibold transition-all cursor-pointer border border-accent/30"
                onclick={() => {
                  selectedKeyPath = key.public_key_path;
                  showCopyModal = true;
                }}
              >
                <Send size={11} />
                <span>Deploy (ssh-copy-id)</span>
              </button>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="py-20 flex flex-col items-center justify-center text-muted border border-dashed border-border rounded-2xl bg-surface-input/10">
        <div class="w-12 h-12 rounded-2xl bg-accent/10 border border-accent/25 flex items-center justify-center text-accent mb-3">
          <KeyRound size={24} />
        </div>
        <h3 class="text-sm font-bold text-primary mb-1">No SSH Keys Found</h3>
        <p class="text-xs text-muted max-w-sm text-center mb-4 leading-relaxed">
          No identity files were detected in ~/.ssh. Generate a modern Ed25519 key pair to start passwordless authentication.
        </p>
        <button
          type="button"
          class="btn btn-primary"
          onclick={() => (showGenerateModal = true)}
        >
          <Plus size={14} />
          <span>Generate New Key</span>
        </button>
      </div>
    {/if}
  </div>
</div>

<!-- Generate Key Modal -->
{#if showGenerateModal}
  <ModalShell
    open={true}
    title="Generate SSH Key Pair"
    onClose={() => (showGenerateModal = false)}
    width="md"
  >
    <div class="px-6 py-4 border-b border-border flex items-center justify-between">
      <h3 class="text-base font-bold text-primary m-0">Generate SSH Key Pair</h3>
      <button
        type="button"
        class="text-muted hover:text-primary p-1 rounded-md border-none bg-transparent cursor-pointer"
        onclick={() => (showGenerateModal = false)}
      >
        <X size={16} />
      </button>
    </div>

    <div class="p-6 flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="gen-key-name" class="text-[11px] font-semibold text-muted uppercase tracking-wider">Key Name</label>
        <input
          id="gen-key-name"
          type="text"
          bind:value={genKeyName}
          class="input font-mono"
          placeholder="id_ed25519_custom"
        />
        <span class="text-[10px] text-muted">Will be saved to ~/.ssh/{genKeyName}</span>
      </div>

      <div class="flex flex-col gap-1.5">
        <span class="text-[11px] font-semibold text-muted uppercase tracking-wider">Cryptographic Algorithm</span>
        <div class="grid grid-cols-2 gap-2">
          <button
            type="button"
            class="p-3 rounded-lg border text-left cursor-pointer transition-all
              {genKeyType === 'ed25519' ? 'border-accent bg-accent/15 text-primary' : 'border-border bg-surface-input text-muted'}"
            onclick={() => (genKeyType = 'ed25519')}
          >
            <div class="font-bold text-xs">Ed25519 (Recommended)</div>
            <div class="text-[10px] text-muted mt-0.5">Modern, high-security 256-bit curve</div>
          </button>
          <button
            type="button"
            class="p-3 rounded-lg border text-left cursor-pointer transition-all
              {genKeyType === 'rsa' ? 'border-accent bg-accent/15 text-primary' : 'border-border bg-surface-input text-muted'}"
            onclick={() => (genKeyType = 'rsa')}
          >
            <div class="font-bold text-xs">RSA 4096-bit</div>
            <div class="text-[10px] text-muted mt-0.5">Legacy compatibility standard</div>
          </button>
        </div>
      </div>

      <div class="flex justify-end gap-2 pt-3 border-t border-border">
        <button
          type="button"
          class="btn btn-secondary"
          onclick={() => (showGenerateModal = false)}
        >
          Cancel
        </button>
        <button
          type="button"
          class="btn btn-primary"
          onclick={handleGenerateKey}
          disabled={generating}
        >
          {#if generating}
            <RefreshCw size={13} class="animate-spin" />
            <span>Generating...</span>
          {:else}
            <span>Generate Key Pair</span>
          {/if}
        </button>
      </div>
    </div>
  </ModalShell>
{/if}

<!-- Copy Key Modal -->
{#if showCopyModal}
  <ModalShell
    open={true}
    title="Deploy SSH Key to Host"
    onClose={() => (showCopyModal = false)}
    width="md"
  >
    <div class="px-6 py-4 border-b border-border flex items-center justify-between">
      <h3 class="text-base font-bold text-primary m-0">Deploy Key (ssh-copy-id)</h3>
      <button
        type="button"
        class="text-muted hover:text-primary p-1 rounded-md border-none bg-transparent cursor-pointer"
        onclick={() => (showCopyModal = false)}
      >
        <X size={16} />
      </button>
    </div>

    <div class="p-6 flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="copy-key-select" class="text-[11px] font-semibold text-muted uppercase tracking-wider">Target Host</label>
        <CustomSelect
          id="copy-key-select"
          options={connections.map((c) => ({ value: `${c.user}@${c.host}`, label: `${c.name} (${c.user}@${c.host})` }))}
          value={selectedTarget}
          onChange={(val) => (selectedTarget = val)}
        />
      </div>

      <div class="p-3 rounded-lg bg-surface-input border border-border/70 text-xs text-secondary leading-relaxed">
        This executes <code class="font-mono text-accent">ssh-copy-id</code> to append this public key to the remote server's <code class="font-mono text-accent">~/.ssh/authorized_keys</code> file, enabling passwordless authentication.
      </div>

      <div class="flex justify-end gap-2 pt-2 border-t border-border">
        <button
          type="button"
          class="btn btn-secondary"
          onclick={() => (showCopyModal = false)}
        >
          Cancel
        </button>
        <button
          type="button"
          class="btn btn-primary"
          onclick={handleCopyKey}
          disabled={copying || !selectedTarget}
        >
          {#if copying}
            <RefreshCw size={13} class="animate-spin" />
            <span>Deploying...</span>
          {:else}
            <Send size={13} />
            <span>Deploy to Host</span>
          {/if}
        </button>
      </div>
    </div>
  </ModalShell>
{/if}
