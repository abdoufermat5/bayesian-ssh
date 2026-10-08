<script lang="ts">
  import { ArrowLeft, Check, FolderOpen, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { OnboardingPayload } from "$lib/types";
  import { invoke } from "@tauri-apps/api/core";
  import { notify } from "$lib/stores/notifications.svelte";
  import { applyTheme, normalizeTheme, type AppTheme } from "$lib/utils/theme";

  interface Props {
    defaultUser: string;
    defaultSshConfigPath: string;
    configRoot: string;
    onBrowseSshConfig: () => Promise<string | null>;
    onComplete: (payload: OnboardingPayload) => Promise<void>;
  }

  let { defaultUser, defaultSshConfigPath, configRoot, onBrowseSshConfig, onComplete }: Props = $props();

  const STEPS = ["Welcome", "Profile", "OpenSSH import", "Appearance", "Done"] as const;
  const LAST = STEPS.length - 1;

  const THEMES: { id: AppTheme; label: string; desc: string }[] = [
    { id: "zinc", label: "Graphite", desc: "Neutral gray, blue accent" },
    { id: "slate", label: "Slate", desc: "Cool blue-gray, sky accent" },
    { id: "cyberpunk", label: "Midnight", desc: "Deep navy, cyan accent" },
    { id: "oled", label: "OLED", desc: "True black, white accent" },
  ];

  let step = $state(0);
  let busy = $state(false);

  // Form fields
  let default_user = $state("");
  let default_port = $state(22);
  let ssh_config_path = $state("");
  let profileName = $state("default");
  let import_ssh_config = $state(true);
  let auto_start_agent = $state(false);
  let fuzzy_search = $state(false);
  let theme = $state<AppTheme>(
    normalizeTheme(typeof document !== "undefined" ? (document.documentElement.dataset.theme ?? "zinc") : "zinc"),
  );

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

  const portInvalid = $derived(!Number.isInteger(default_port) || default_port < 1 || default_port > 65535);
  const profileTrimmed = $derived(profileName.trim() || "default");

  function next() {
    if (step === 1 && portInvalid) return;
    if (step < LAST) step += 1;
  }

  function back() {
    if (step > 0) step -= 1;
  }

  function pickTheme(id: AppTheme) {
    theme = id;
    applyTheme(id); // live preview; persisted by complete_onboarding
  }

  async function browseConfig() {
    const picked = await onBrowseSshConfig();
    if (picked) ssh_config_path = picked;
  }

  async function finish() {
    busy = true;
    try {
      await onComplete({
        profile_name: profileTrimmed,
        create_profile: profileTrimmed !== "default",
        default_user: default_user.trim() || defaultUser || "root",
        default_port: portInvalid ? 22 : default_port,
        ssh_config_path: ssh_config_path.trim() || null,
        theme,
        auto_start_agent,
        import_ssh_config,
        fuzzy_search,
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
      restorePassphrase = "";
      await onComplete({
        profile_name: "default",
        create_profile: false,
        default_user: default_user || defaultUser || "root",
        default_port: 22,
        ssh_config_path: null,
        theme,
        auto_start_agent: false,
        import_ssh_config: false,
        fuzzy_search: false,
      });
      notify(`Backup restored (${count} connection(s) loaded)`, "success");
    } catch (err: unknown) {
      const errStr = String(err);
      if (errStr.includes("requires passphrase") || errStr.includes("decrypt") || errStr.includes("Passphrase")) {
        if (pass !== null) notify(`Restore failed: ${err}`, "error");
        showPassphraseInput = true;
      } else {
        notify(`Restore failed: ${err}`, "error");
      }
    } finally {
      busy = false;
    }
  }

  async function submitPassphrase(e?: SubmitEvent) {
    e?.preventDefault();
    // Send the passphrase verbatim: export (Settings → Profiles) does not
    // trim, so trimming here would make backups with edge whitespace unrestorable.
    if (!restorePassphrase || !selectedBackupPath) return;
    await executeRestore(selectedBackupPath, restorePassphrase);
  }

  // Skip setup entirely: complete onboarding with minimal defaults.
  function handleSkip() {
    onComplete({
      profile_name: "default",
      create_profile: false,
      default_user: default_user.trim() || defaultUser || "root",
      default_port: portInvalid ? 22 : default_port,
      ssh_config_path: null,
      theme,
      auto_start_agent: false,
      import_ssh_config: false,
      fuzzy_search: false,
    });
  }

  function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    if (step === LAST) void finish();
    else next();
  }

  // Move focus to the first field of each step (or the primary button).
  function focusStep(node: HTMLElement) {
    requestAnimationFrame(() => {
      const target =
        node.querySelector<HTMLElement>("[data-autofocus]") ??
        node.querySelector<HTMLElement>("input:not([type=checkbox])") ??
        document.getElementById("ob-primary");
      target?.focus({ preventScroll: true });
    });
  }
</script>

<div
  class="fixed inset-x-0 bottom-0 top-[var(--titlebar-h,36px)] z-[90] flex overflow-y-auto bg-surface"
  role="main"
  aria-label="First-run setup"
>
  <button type="button" class="btn btn-ghost btn-sm absolute right-4 top-3" onclick={handleSkip} disabled={busy}>
    Skip setup
  </button>

  <!-- Anchored near the top (not centered) so the stepper doesn't jump as step heights change. -->
  <div class="mx-auto mb-10 mt-[14vh] flex h-fit w-full max-w-[520px] flex-col gap-5 px-6">
    <!-- Stepper -->
    <ol class="m-0 grid list-none grid-cols-5 gap-2 p-0" aria-label="Setup progress">
      {#each STEPS as label, i (label)}
        <li class="flex min-w-0 flex-col gap-1.5" aria-current={i === step ? "step" : undefined}>
          <span class="h-0.5 rounded-full {i <= step ? 'bg-accent' : 'bg-surface-active'}"></span>
          <span class="truncate text-xs {i === step ? 'text-primary' : i < step ? 'text-secondary' : 'text-muted'}">
            {label}
          </span>
        </li>
      {/each}
    </ol>

    <form class="flex flex-col rounded-xl border border-border bg-panel" novalidate onsubmit={handleSubmit}>
      {#key step}
        <div class="flex flex-col gap-5 p-6" use:focusStep>
          {#if step === 0}
            <div class="flex flex-col gap-3">
              <svg viewBox="0 0 20 20" class="size-10" aria-hidden="true">
                <rect width="20" height="20" rx="5" fill="var(--color-accent)" />
                <path d="M5.5 7l3 3-3 3" fill="none" stroke="var(--color-on-accent)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
                <path d="M10.5 13.25h4" stroke="var(--color-on-accent)" stroke-width="1.6" stroke-linecap="round" />
              </svg>
              <div class="flex flex-col gap-1.5">
                <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Welcome to Bayesian SSH</h1>
                <p class="m-0 text-sm leading-relaxed text-secondary">
                  A keyboard-first manager for your SSH hosts. Setup takes under a minute: pick your defaults,
                  import hosts from OpenSSH and choose a theme.
                </p>
              </div>
            </div>
            <div class="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2.5">
              <span class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm text-primary">Moving from another machine?</span>
                <span class="text-xs text-muted">Restore hosts from a backup file (.json or .enc).</span>
              </span>
              <button type="button" class="btn btn-secondary btn-sm" onclick={handleRestoreBackup} disabled={busy}>
                Restore backup
              </button>
            </div>
          {:else if step === 1}
            <div class="flex flex-col gap-1">
              <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Profile and defaults</h1>
              <p class="m-0 text-sm text-muted">Used when a host doesn't set its own user or port.</p>
            </div>
            <div class="flex flex-col gap-4">
              <div class="field">
                <label for="ob-profile" class="field-label">Profile name</label>
                <input
                  id="ob-profile"
                  type="text"
                  class="input"
                  placeholder="default"
                  autocomplete="off"
                  spellcheck="false"
                  bind:value={profileName}
                />
                <span class="field-hint">Profiles keep separate host lists, e.g. work and personal.</span>
              </div>
              <div class="grid grid-cols-[1fr_96px] gap-3">
                <div class="field">
                  <label for="ob-user" class="field-label">Default user</label>
                  <input
                    id="ob-user"
                    type="text"
                    class="input input-mono"
                    autocomplete="off"
                    spellcheck="false"
                    bind:value={default_user}
                  />
                </div>
                <div class="field">
                  <label for="ob-port" class="field-label">Default port</label>
                  <input
                    id="ob-port"
                    type="number"
                    min="1"
                    max="65535"
                    class="input input-mono tabular-nums"
                    class:input-invalid={portInvalid}
                    bind:value={default_port}
                  />
                </div>
              </div>
              {#if portInvalid}
                <span class="field-error -mt-2">Port must be between 1 and 65535.</span>
              {/if}
              <label for="ob-agent" class="flex cursor-pointer items-center justify-between gap-4">
                <span class="flex flex-col gap-0.5">
                  <span class="text-sm text-primary">Start the SSH agent on launch</span>
                  <span class="text-xs text-muted">Keeps added keys available to every session.</span>
                </span>
                <input id="ob-agent" type="checkbox" class="switch" bind:checked={auto_start_agent} />
              </label>
            </div>
          {:else if step === 2}
            <div class="flex flex-col gap-1">
              <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Import from OpenSSH</h1>
              <p class="m-0 text-sm text-muted">Bring in the hosts you already have in your SSH config.</p>
            </div>
            <div class="flex flex-col gap-4">
              <label for="ob-import" class="flex cursor-pointer items-center justify-between gap-4">
                <span class="flex flex-col gap-0.5">
                  <span class="text-sm text-primary">Import hosts</span>
                  <span class="text-xs text-muted">Hosts are copied into this profile. Your config file is only read.</span>
                </span>
                <input id="ob-import" type="checkbox" class="switch" bind:checked={import_ssh_config} />
              </label>
              <div class="field">
                <label for="ob-ssh-config" class="field-label">Config file</label>
                <div class="flex gap-2">
                  <input
                    id="ob-ssh-config"
                    type="text"
                    class="input input-mono flex-1"
                    placeholder="~/.ssh/config"
                    spellcheck="false"
                    bind:value={ssh_config_path}
                    disabled={!import_ssh_config}
                  />
                  <button type="button" class="btn btn-secondary" onclick={browseConfig} disabled={!import_ssh_config}>
                    <FolderOpen size={14} />
                    Browse
                  </button>
                </div>
              </div>
            </div>
          {:else if step === 3}
            <div class="flex flex-col gap-1">
              <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Appearance</h1>
              <p class="m-0 text-sm text-muted">You can change the theme later in Settings.</p>
            </div>
            <div class="grid grid-cols-2 gap-2" role="radiogroup" aria-label="Theme">
              {#each THEMES as t (t.id)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={theme === t.id}
                  class="card card-interactive flex items-start justify-between gap-2 px-3 py-2.5 text-left {theme === t.id ? 'card-selected' : ''}"
                  onclick={() => pickTheme(t.id)}
                  data-autofocus={theme === t.id ? true : undefined}
                >
                  <span class="flex min-w-0 flex-col gap-0.5">
                    <span class="text-sm text-primary">{t.label}</span>
                    <span class="truncate text-xs text-muted">{t.desc}</span>
                  </span>
                  {#if theme === t.id}<Check size={14} class="mt-0.5 shrink-0 text-accent" />{/if}
                </button>
              {/each}
            </div>
            <label for="ob-fuzzy" class="flex cursor-pointer items-center justify-between gap-4">
              <span class="flex flex-col gap-0.5">
                <span class="text-sm text-primary">Fuzzy host search</span>
                <span class="text-xs text-muted">Match typos and partial host names.</span>
              </span>
              <input id="ob-fuzzy" type="checkbox" class="switch" bind:checked={fuzzy_search} />
            </label>
          {:else}
            <div class="flex flex-col gap-1">
              <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">You're all set</h1>
              <p class="m-0 text-sm text-muted">Review your choices. Everything can be changed in Settings.</p>
            </div>
            <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 rounded-md border border-border px-3 py-2.5 text-sm">
              <dt class="text-muted">Profile</dt>
              <dd class="m-0 truncate text-primary">{profileTrimmed}</dd>
              <dt class="text-muted">Defaults</dt>
              <dd class="m-0 truncate font-mono text-xs leading-5 text-primary">
                {default_user.trim() || defaultUser || "root"}:{portInvalid ? 22 : default_port}
              </dd>
              <dt class="text-muted">SSH agent</dt>
              <dd class="m-0 text-primary">{auto_start_agent ? "Starts on launch" : "Manual"}</dd>
              <dt class="text-muted">OpenSSH import</dt>
              <dd class="m-0 truncate text-primary" title={ssh_config_path}>
                {import_ssh_config ? (ssh_config_path.trim() || "~/.ssh/config") : "Skipped"}
              </dd>
              <dt class="text-muted">Theme</dt>
              <dd class="m-0 text-primary">{THEMES.find((t) => t.id === theme)?.label}</dd>
            </dl>
          {/if}
        </div>
      {/key}

      <div class="flex items-center gap-2 border-t border-border px-6 py-3">
        {#if step > 0}
          <button type="button" class="btn btn-ghost" onclick={back} disabled={busy}>
            <ArrowLeft size={14} />
            Back
          </button>
        {/if}
        <span class="ml-auto text-xs tabular-nums text-muted">Step {step + 1} of {STEPS.length}</span>
        <button
          id="ob-primary"
          type="submit"
          class="btn btn-primary"
          disabled={busy || (step === 1 && portInvalid)}
        >
          {#if busy}<span class="spinner"></span>{/if}
          {step === 0 ? "Get started" : step === LAST ? "Open Bayesian SSH" : "Continue"}
        </button>
      </div>
    </form>

    {#if configRoot}
      <p class="m-0 truncate text-center font-mono text-xs text-muted" title={configRoot}>{configRoot}</p>
    {/if}
  </div>
</div>

<ModalShell
  open={showPassphraseInput}
  title="Encrypted backup"
  onClose={() => (showPassphraseInput = false)}
  closeOnBackdrop={false}
  width="sm"
>
  <form class="flex min-h-0 flex-1 flex-col" novalidate onsubmit={submitPassphrase}>
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Encrypted backup</h2>
        <p class="modal-subtitle">Enter the passphrase used when the backup was exported.</p>
      </div>
      <button type="button" class="modal-close" onclick={() => (showPassphraseInput = false)} aria-label="Close">
        <X size={16} />
      </button>
    </div>
    <div class="modal-body">
      <div class="field">
        <label for="ob-passphrase" class="field-label">Passphrase</label>
        <input
          id="ob-passphrase"
          type="password"
          class="input"
          autocomplete="off"
          bind:value={restorePassphrase}
        />
      </div>
    </div>
    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={() => (showPassphraseInput = false)}>Cancel</button>
      <button type="submit" class="btn btn-primary" disabled={busy || !restorePassphrase}>
        {#if busy}<span class="spinner"></span>{/if}
        Restore
      </button>
    </div>
  </form>
</ModalShell>
