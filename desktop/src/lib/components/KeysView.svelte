<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Check, Copy, KeyRound, Plus, RefreshCw, Send, ShieldAlert, X } from "lucide-svelte";
  import type { Connection, SshKeyInfo } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";

  interface Props {
    connections: Connection[];
  }

  let { connections }: Props = $props();

  type KeyType = "ed25519" | "rsa";
  const DEFAULT_KEY_NAME = "id_ed25519_bssh";
  const KEY_TYPES: { value: KeyType; label: string; desc: string }[] = [
    { value: "ed25519", label: "Ed25519", desc: "Recommended. Short keys, fast and secure." },
    { value: "rsa", label: "RSA 4096", desc: "For older servers without Ed25519 support." },
  ];

  let keys = $state.raw<SshKeyInfo[]>([]);
  let loading = $state(true);
  let copiedText = $state<string | null>(null);

  // Generate key dialog
  let showGenerateModal = $state(false);
  let genKeyName = $state(DEFAULT_KEY_NAME);
  let genKeyType = $state<KeyType>("ed25519");
  let generating = $state(false);

  // Deploy (ssh-copy-id) dialog
  let showCopyModal = $state(false);
  let selectedKey = $state<SshKeyInfo | null>(null);
  let selectedTarget = $state("");
  let copying = $state(false);

  const insecureKeys = $derived(keys.filter((k) => !k.is_secure));
  const fixCommand = $derived(
    insecureKeys
      .map((k) => `chmod 600 ${shortPath(k.private_key_path ?? k.public_key_path)}`)
      .join(" && "),
  );
  const hostOptions = $derived(
    connections.map((c) => ({ value: `${c.user}@${c.host}`, label: `${c.name} (${c.user}@${c.host})` })),
  );

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

  function openGenerate() {
    genKeyName = DEFAULT_KEY_NAME;
    genKeyType = "ed25519";
    showGenerateModal = true;
  }

  async function handleGenerateKey(e?: SubmitEvent) {
    e?.preventDefault();
    const name = genKeyName.trim();
    if (!name) {
      notify("Key name is required", "error");
      return;
    }
    generating = true;
    try {
      await invoke("generate_ssh_key", { name, keyType: genKeyType });
      notify(`Generated ${name}`, "success");
      showGenerateModal = false;
      await loadKeys();
    } catch (err) {
      notify(`Key generation failed: ${err}`, "error");
    } finally {
      generating = false;
    }
  }

  function openDeploy(key: SshKeyInfo) {
    selectedKey = key;
    showCopyModal = true;
  }

  async function handleCopyKey(e?: SubmitEvent) {
    e?.preventDefault();
    if (!selectedTarget) {
      notify("Choose a host first", "error");
      return;
    }
    copying = true;
    try {
      const msg = await invoke<string>("copy_ssh_key_to_target", {
        target: selectedTarget,
        keyPath: selectedKey?.public_key_path || null,
      });
      notify(msg, "success");
      showCopyModal = false;
    } catch (err) {
      notify(`Deploying key failed: ${err}`, "error");
    } finally {
      copying = false;
    }
  }

  function copyToClipboard(text: string, what: string) {
    copyTextWithFallback(text);
    copiedText = text;
    notify(`Copied ${what}`, "success");
    setTimeout(() => {
      if (copiedText === text) copiedText = null;
    }, 2000);
  }

  /** `/home/me/.ssh/id_x` → `~/.ssh/id_x` for display; the title keeps the full path. */
  function shortPath(path: string): string {
    return path.replace(/^\/(?:home|Users)\/[^/]+(?=\/)/, "~");
  }
</script>

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Keys</h1>
      {#if !loading || keys.length > 0}
        <span class="text-sm tabular-nums text-muted">{keys.length}</span>
      {/if}
    </div>
    <div class="view-actions">
      <button
        type="button"
        class="btn-icon"
        onclick={loadKeys}
        disabled={loading}
        aria-label="Refresh keys"
        title="Refresh"
      >
        <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
      </button>
      <button type="button" class="btn btn-primary" onclick={openGenerate}>
        <Plus size={15} />
        Generate key
      </button>
    </div>
  </header>

  <div class="view-body">
    {#if loading && keys.length === 0}
      <div class="table-wrap" aria-busy="true" aria-label="Loading keys">
        {#each [0, 1, 2] as i (i)}
          <div class="flex h-14 items-center gap-4 px-4 {i > 0 ? 'border-t border-border-subtle' : ''}">
            <div class="flex w-[30%] flex-col gap-1.5">
              <div class="skeleton h-3 w-28"></div>
              <div class="skeleton h-2.5 w-40"></div>
            </div>
            <div class="skeleton h-4 w-16"></div>
            <div class="skeleton h-3 flex-1"></div>
            <div class="skeleton h-4 w-12"></div>
            <div class="skeleton h-7 w-20"></div>
          </div>
        {/each}
      </div>
    {:else if keys.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><KeyRound size={18} /></div>
        <div class="empty-state-title">No SSH keys</div>
        <p class="empty-state-desc">No identity files were found in ~/.ssh. Generate a key pair to get started.</p>
        <div class="empty-state-action">
          <button type="button" class="btn btn-primary" onclick={openGenerate}>
            <Plus size={15} />
            Generate key
          </button>
        </div>
      </div>
    {:else}
      {#if insecureKeys.length > 0}
        <div class="alert alert-error mb-3" role="alert">
          <ShieldAlert size={14} class="mt-0.5 shrink-0" />
          <div class="flex min-w-0 flex-1 flex-col gap-1.5">
            <span>
              <span class="alert-title">
                {insecureKeys.length === 1 ? "1 private key is" : `${insecureKeys.length} private keys are`} readable by other users.
              </span>
              <span class="text-secondary">OpenSSH refuses keys like this. Restrict them to your user:</span>
            </span>
            <div class="flex min-w-0 items-center gap-1">
              <code class="min-w-0 truncate font-mono text-xs text-primary select-text" title={fixCommand}>{fixCommand}</code>
              <button
                type="button"
                class="btn-icon btn-icon-sm"
                onclick={() => copyToClipboard(fixCommand, "command")}
                aria-label="Copy fix command"
                title="Copy command"
              >
                {#if copiedText === fixCommand}<Check size={13} class="text-success" />{:else}<Copy size={13} />{/if}
              </button>
            </div>
          </div>
        </div>
      {/if}

      <div class="table-wrap">
        <table class="data-table table-fixed">
          <thead>
            <tr>
              <th class="w-[34%]">Key</th>
              <th class="w-[112px]">Type</th>
              <th>Fingerprint</th>
              <th class="w-[112px]">Permissions</th>
              <th class="w-[112px]"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each keys as key (key.public_key_path)}
              {@const path = key.private_key_path ?? key.public_key_path}
              <tr>
                <td class="py-2">
                  <div class="flex min-w-0 flex-col gap-0.5">
                    <div class="flex min-w-0 items-baseline gap-2">
                      <span class="truncate font-medium text-primary" title={key.name}>{key.name}</span>
                      {#if key.comment}
                        <span class="truncate text-xs text-muted" title={key.comment}>{key.comment}</span>
                      {/if}
                    </div>
                    <span
                      class="truncate font-mono text-xs text-muted"
                      title={key.private_key_path
                        ? `${key.private_key_path}\n${key.public_key_path}`
                        : key.public_key_path}
                    >
                      {shortPath(path)}{key.private_key_path && key.public_key_path !== `${key.private_key_path}.pub`
                        ? ` · ${shortPath(key.public_key_path)}`
                        : ""}
                    </span>
                  </div>
                </td>
                <td>
                  <span class="badge badge-neutral badge-mono">{key.key_type}</span>
                </td>
                <td>
                  <div class="flex min-w-0 items-center gap-1">
                    <span class="truncate font-mono text-xs text-secondary select-text" title={key.fingerprint}>
                      {key.fingerprint}
                    </span>
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      onclick={() => copyToClipboard(key.fingerprint, "fingerprint")}
                      aria-label="Copy fingerprint of {key.name}"
                      title="Copy fingerprint"
                    >
                      {#if copiedText === key.fingerprint}<Check size={13} class="text-success" />{:else}<Copy size={13} />{/if}
                    </button>
                  </div>
                </td>
                <td>
                  {#if key.is_secure}
                    <span class="badge badge-success badge-mono" title="Private key is only readable by you">
                      {key.private_key_permission}
                    </span>
                  {:else}
                    <span
                      class="badge badge-error badge-mono"
                      title="Readable by other users. Fix with: chmod 600 {shortPath(path)}"
                    >
                      <ShieldAlert size={11} />
                      {key.private_key_permission}
                    </span>
                  {/if}
                </td>
                <td>
                  <div class="flex justify-end">
                    <button
                      type="button"
                      class="btn btn-secondary btn-sm"
                      onclick={() => openDeploy(key)}
                      disabled={connections.length === 0}
                      title={connections.length === 0
                        ? "Add a host first"
                        : "Copy this public key to a host's authorized_keys"}
                    >
                      <Send size={13} />
                      Deploy
                    </button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>

{#if showGenerateModal}
  <ModalShell open={true} title="Generate key" onClose={() => (showGenerateModal = false)} width="md">
    <form class="flex min-h-0 flex-1 flex-col" onsubmit={handleGenerateKey}>
      <div class="modal-header">
        <div class="min-w-0">
          <h2 class="modal-title">Generate key</h2>
          <p class="modal-subtitle">Creates a new key pair in ~/.ssh.</p>
        </div>
        <button
          type="button"
          class="modal-close"
          onclick={() => (showGenerateModal = false)}
          aria-label="Close"
        >
          <X size={16} />
        </button>
      </div>

      <div class="modal-body flex flex-col gap-4">
        <div class="field">
          <label for="gen-key-name" class="field-label">File name</label>
          <input
            id="gen-key-name"
            type="text"
            bind:value={genKeyName}
            class="input input-mono"
            placeholder="id_ed25519_work"
            spellcheck="false"
            autocomplete="off"
          />
          <span class="field-hint">
            Saved as <span class="font-mono">~/.ssh/{genKeyName.trim() || "…"}</span> and
            <span class="font-mono">.pub</span>
          </span>
        </div>

        <div class="field" role="radiogroup" aria-labelledby="gen-key-type-label">
          <span id="gen-key-type-label" class="field-label">Algorithm</span>
          <div class="grid grid-cols-2 gap-2">
            {#each KEY_TYPES as type (type.value)}
              {@const active = genKeyType === type.value}
              <button
                type="button"
                role="radio"
                aria-checked={active}
                class="card card-interactive flex flex-col gap-0.5 p-3 text-left {active ? 'card-selected' : ''}"
                onclick={() => (genKeyType = type.value)}
              >
                <span class="text-sm font-medium text-primary">{type.label}</span>
                <span class="text-xs leading-snug text-muted">{type.desc}</span>
              </button>
            {/each}
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick={() => (showGenerateModal = false)}>
          Cancel
        </button>
        <button type="submit" class="btn btn-primary" disabled={generating || !genKeyName.trim()}>
          {#if generating}<span class="spinner size-3"></span>Generating…{:else}Generate key{/if}
        </button>
      </div>
    </form>
  </ModalShell>
{/if}

{#if showCopyModal}
  <ModalShell open={true} title="Deploy key" onClose={() => (showCopyModal = false)} width="md">
    <form class="flex min-h-0 flex-1 flex-col" onsubmit={handleCopyKey}>
      <div class="modal-header">
        <div class="min-w-0">
          <h2 class="modal-title">Deploy key</h2>
          <p class="modal-subtitle truncate">
            {#if selectedKey}
              <span class="font-mono" title={selectedKey.public_key_path}>{shortPath(selectedKey.public_key_path)}</span>
            {:else}
              Default public key
            {/if}
          </p>
        </div>
        <button type="button" class="modal-close" onclick={() => (showCopyModal = false)} aria-label="Close">
          <X size={16} />
        </button>
      </div>

      <div class="modal-body flex flex-col gap-4">
        <div class="field">
          <label for="copy-key-select" class="field-label">Host</label>
          <CustomSelect
            id="copy-key-select"
            options={hostOptions}
            value={selectedTarget}
            onChange={(val) => (selectedTarget = val)}
            placeholder="Choose a host"
          />
          <span class="field-hint">
            Runs <span class="font-mono text-secondary">ssh-copy-id</span>, which appends the public key to
            <span class="font-mono text-secondary">~/.ssh/authorized_keys</span> on the host.
          </span>
        </div>
      </div>

      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick={() => (showCopyModal = false)}>Cancel</button>
        <button type="submit" class="btn btn-primary" disabled={copying || !selectedTarget}>
          {#if copying}<span class="spinner size-3"></span>Deploying…{:else}Deploy key{/if}
        </button>
      </div>
    </form>
  </ModalShell>
{/if}
