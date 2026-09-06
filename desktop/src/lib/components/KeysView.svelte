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

<div class="flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-surface text-[13px] text-primary select-none">
  <div class="view-header">
    <div class="flex min-w-0 items-center gap-3">
      <div class="icon-tile rounded-md">
        <KeyRound size={16} />
      </div>
      <div class="min-w-0">
        <h2 class="m-0 flex items-center gap-2 truncate text-sm font-bold tracking-tight text-primary">
          SSH Keys
          <span class="badge badge-subtle">{keys.length} keys</span>
        </h2>
        <p class="m-0 truncate text-xs text-muted">Local identities, fingerprints, permissions, and deployment</p>
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

  <div class="view-content">
    {#if loading}
      <div class="empty-state">
        <RefreshCw size={24} class="animate-spin text-accent" />
        <span class="text-xs font-semibold text-primary">Scanning ~/.ssh identities...</span>
      </div>
    {:else if keys.length > 0}
      <div class="grid grid-cols-1 gap-3 xl:grid-cols-2">
        {#each keys as key}
          <section class="panel flex min-w-0 flex-col overflow-hidden">
            <div class="flex items-start justify-between gap-3 border-b border-border/70 px-4 py-3">
              <div class="flex min-w-0 items-center gap-2">
                <KeyRound size={14} class="shrink-0 text-accent" />
                <div class="min-w-0">
                  <h3 class="m-0 truncate text-sm font-semibold text-primary">{key.name}</h3>
                  <p class="m-0 truncate font-mono text-xs text-muted">{key.comment || "No comment"}</p>
                </div>
              </div>
              <div class="flex shrink-0 items-center gap-1.5">
                <span class="tag uppercase">{key.key_type}</span>
                  {#if key.is_secure}
                  <span class="badge badge-success">
                    <ShieldCheck size={10} /> 0600
                    </span>
                  {:else}
                  <span class="badge badge-error">
                    <ShieldAlert size={10} /> {key.private_key_permission}
                    </span>
                  {/if}
              </div>
            </div>

            <div class="grid gap-2 px-4 py-3">
              <div class="flex min-w-0 items-center gap-2 rounded-md border border-border/80 bg-surface-input px-2.5 py-2">
                <span class="w-20 shrink-0 table-header">Fingerprint</span>
                <span class="min-w-0 flex-1 truncate font-mono text-xs text-muted">{key.fingerprint}</span>
                <button
                  type="button"
                  class="btn-icon shrink-0"
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

              <div class="grid gap-1 font-mono text-xs text-muted">
                <div class="flex min-w-0 items-center gap-2">
                  <span class="w-16 shrink-0 table-header">Public</span>
                  <span class="min-w-0 flex-1 truncate" title={key.public_key_path}>
                    {key.public_key_path}
                  </span>
                </div>
                {#if key.private_key_path}
                  <div class="flex min-w-0 items-center gap-2">
                    <span class="w-16 shrink-0 table-header">Private</span>
                    <span class="min-w-0 flex-1 truncate" title={key.private_key_path}>
                      {key.private_key_path}
                    </span>
                  </div>
                {/if}
              </div>
            </div>

            <div class="flex items-center justify-end gap-2 border-t border-border/60 px-4 py-3">
              <button
                type="button"
                class="btn btn-secondary"
                onclick={() => {
                  selectedKeyPath = key.public_key_path;
                  showCopyModal = true;
                }}
              >
                <Send size={11} />
                <span>Deploy</span>
              </button>
            </div>
          </section>
        {/each}
      </div>
    {:else}
      <div class="empty-state empty-state-dashed">
        <div class="empty-state-icon">
          <KeyRound size={22} />
        </div>
        <h3 class="empty-state-title">No SSH keys found</h3>
        <p class="empty-state-desc">No identity files were detected in ~/.ssh.</p>
        <button
          type="button"
          class="btn btn-primary empty-state-action"
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
    <div class="modal-header">
      <div>
        <h3 class="modal-title">Generate SSH Key Pair</h3>
        <p class="modal-subtitle">Create a new cryptographic key pair in ~/.ssh</p>
      </div>
      <button
        type="button"
        class="modal-close-btn"
        onclick={() => (showGenerateModal = false)}
        aria-label="Close dialog"
      >
        <X size={16} />
      </button>
    </div>

    <div class="modal-body space-y-4">
      <div class="flex flex-col gap-1.5">
        <label for="gen-key-name" class="table-header">Key Name</label>
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
        <span class="table-header">Cryptographic Algorithm</span>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
          <button
            type="button"
            class="p-3 rounded-lg border text-left cursor-pointer transition-all
              {genKeyType === 'ed25519' ? 'border-accent/50 bg-surface-active text-primary' : 'border-border bg-surface-input text-muted hover:border-border-strong'}"
            onclick={() => (genKeyType = 'ed25519')}
          >
            <div class="font-semibold text-xs text-primary">Ed25519 (Recommended)</div>
            <div class="text-[10px] text-muted mt-0.5">Modern, high-security 256-bit curve</div>
          </button>
          <button
            type="button"
            class="p-3 rounded-lg border text-left cursor-pointer transition-all
              {genKeyType === 'rsa' ? 'border-accent/50 bg-surface-active text-primary' : 'border-border bg-surface-input text-muted hover:border-border-strong'}"
            onclick={() => (genKeyType = 'rsa')}
          >
            <div class="font-semibold text-xs text-primary">RSA 4096-bit</div>
            <div class="text-[10px] text-muted mt-0.5">Legacy compatibility standard</div>
          </button>
        </div>
      </div>
    </div>

    <div class="modal-footer">
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
    <div class="modal-header">
      <div>
        <h3 class="modal-title">Deploy Key (ssh-copy-id)</h3>
        <p class="modal-subtitle">Authorize this public key on a remote server</p>
      </div>
      <button
        type="button"
        class="modal-close-btn"
        onclick={() => (showCopyModal = false)}
        aria-label="Close dialog"
      >
        <X size={16} />
      </button>
    </div>

    <div class="modal-body space-y-4">
      <div class="flex flex-col gap-1.5">
        <label for="copy-key-select" class="table-header">Target Host</label>
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
    </div>

    <div class="modal-footer">
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
  </ModalShell>
{/if}
