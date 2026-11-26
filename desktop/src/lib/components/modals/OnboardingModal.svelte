<script lang="ts">
  import { ArrowLeft, Check, FolderOpen, X } from "lucide-svelte";
  import BrandMark from "$lib/components/BrandMark.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { OnboardingPayload } from "$lib/types";
  import { invoke } from "@tauri-apps/api/core";
  import { notify } from "$lib/stores/notifications.svelte";
  import { applyTheme, normalizeTheme, THEME_SWATCHES, type AppTheme } from "$lib/utils/theme";
  import { HOST_SHORTCUTS } from "$lib/shortcuts";
  import { appState } from "$lib/stores/appState.svelte";
  import { requestTeamFocus } from "$lib/stores/attention.svelte";
  import InvitedPath from "$lib/components/onboarding/InvitedPath.svelte";
  import type { TeamJoin } from "$lib/types";

  interface Props {
    defaultUser: string;
    defaultSshConfigPath: string;
    configRoot: string;
    onBrowseSshConfig: () => Promise<string | null>;
    onComplete: (payload: OnboardingPayload) => Promise<void>;
  }

  let { defaultUser, defaultSshConfigPath, configRoot, onBrowseSshConfig, onComplete }: Props = $props();

  const STEPS = ["Welcome", "Profile & import", "Appearance"] as const;
  const LAST = STEPS.length - 1;

  const FALLBACK_SWATCHES = {
    light: {
      chrome: "#f1f3f5",
      surface: "#f8f9fa",
      panel: "#ffffff",
      border: "#d8dee6",
      text: "#111418",
      muted: "#5b6470",
      accent: "#2563eb",
    },
    dark: {
      chrome: "#0b0d10",
      surface: "#111318",
      panel: "#171a21",
      border: "#272c36",
      text: "#eceff4",
      muted: "#8c96a6",
      accent: "#3b82f6",
    },
  } as const;

  const swatches = THEME_SWATCHES ?? FALLBACK_SWATCHES;

  const THEME_CARDS: {
    id: AppTheme;
    name: string;
    chrome: string;
    surface: string;
    panel: string;
    border: string;
    text: string;
    muted: string;
    accent: string;
  }[] = [
    { id: "light", name: "Light", ...swatches.light },
    { id: "dark", name: "Dark", ...swatches.dark },
    { id: "system", name: "Match system", chrome: "", surface: "", panel: "", border: "", text: "", muted: "", accent: "" },
  ];

  const CONNECT = HOST_SHORTCUTS.find((s) => s.id === "host-connect");
  const CONNECT_CLI = CONNECT?.cli ?? "bssh connect <host>";
  const CONNECT_KEY = CONNECT?.combos[0].join("+") ?? "Enter";

  let step = $state(0);
  /** Which welcome choice applies; "invite" swaps the stepper for the invited-member path. */
  let path = $state<"machine" | "invite">("machine");
  let inInvitePath = $state(false);
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
    normalizeTheme(typeof document !== "undefined" ? (document.documentElement.dataset.theme ?? "dark") : "dark"),
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
    if (step === 0 && path === "invite") {
      inInvitePath = true;
      return;
    }
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

  async function finishInvited(join: TeamJoin) {
    busy = true;
    try {
      await onComplete({
        profile_name: "default",
        create_profile: false,
        default_user: default_user.trim() || defaultUser || "root",
        default_port: 22,
        ssh_config_path: null,
        theme,
        auto_start_agent: false,
        import_ssh_config: false,
        fuzzy_search: false,
      });
      if (!appState.showOnboarding) {
        appState.activeTab = "teams";
        requestTeamFocus(join.team_id);
      }
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
  class="fixed inset-x-0 bottom-0 top-[var(--titlebar-h,38px)] z-[90] flex overflow-y-auto bg-surface"
  role="main"
  aria-label="First-run setup"
>
  <button type="button" class="btn btn-ghost btn-sm absolute right-4 top-3" onclick={handleSkip} disabled={busy}>
    Skip setup
  </button>

  <!-- Anchored near the top (not centered) so the stepper doesn't jump as step heights change. -->
  <div class="mx-auto mb-10 mt-[10vh] flex h-fit w-full max-w-[540px] flex-col gap-5 px-6">
    {#if inInvitePath}
      <InvitedPath {busy} onBack={() => (inInvitePath = false)} onFinish={finishInvited} />
    {:else}
    <!-- Stepper -->
    <ol class="m-0 grid list-none grid-cols-3 gap-2 p-0" aria-label="Setup progress">
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
              <BrandMark class="size-10" />
              <div class="flex flex-col gap-1.5">
                <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Welcome to Bayesian SSH</h1>
                <p class="m-0 text-sm leading-relaxed text-secondary">
                  A keyboard-first manager for your SSH hosts. Which of these applies?
                </p>
              </div>
            </div>
            <div
              class="flex flex-col gap-2"
              role="radiogroup"
              tabindex="-1"
              aria-label="What brings you here"
              onkeydown={(e) => {
                const opts: Array<"machine" | "invite"> = ["machine", "invite"];
                const idx = opts.indexOf(path);
                let next = -1;
                if (e.key === "ArrowDown" || e.key === "ArrowRight") next = (idx + 1) % opts.length;
                else if (e.key === "ArrowUp" || e.key === "ArrowLeft") next = (idx - 1 + opts.length) % opts.length;
                if (next === -1) return;
                e.preventDefault();
                path = opts[next];
                const buttons = (e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('[role="radio"]');
                buttons[next]?.focus();
              }}
            >
              {#each [{ id: "machine", label: "Set up this machine", desc: "Pick defaults, add or import your hosts, connect." }, { id: "invite", label: "I was invited to a team", desc: "Paste the invite and join your team's shared hosts." }] as opt (opt.id)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={path === opt.id}
                  tabindex={path === opt.id ? 0 : -1}
                  class="card card-interactive flex items-start justify-between gap-2 px-3 py-2.5 text-left {path === opt.id ? 'card-selected' : ''}"
                  onclick={() => (path = opt.id as "machine" | "invite")}
                  data-autofocus={path === opt.id ? true : undefined}
                >
                  <span class="flex min-w-0 flex-col gap-0.5">
                    <span class="text-sm text-primary">{opt.label}</span>
                    <span class="text-xs text-muted">{opt.desc}</span>
                  </span>
                  {#if path === opt.id}<Check size={14} class="mt-0.5 shrink-0 text-accent" />{/if}
                </button>
              {/each}
            </div>
            <div class="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2.5">
              <span class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm text-primary">Moving from another machine?</span>
                <span class="text-xs text-muted">Restore hosts from a backup file (.json or .enc).</span>
              </span>
              <button type="button" class="btn btn-secondary btn-sm shrink-0" onclick={handleRestoreBackup} disabled={busy}>
                Restore backup
              </button>
            </div>
          {:else if step === 1}
            <div class="flex flex-col gap-1">
              <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Profile &amp; import</h1>
              <p class="m-0 text-sm text-muted">Set connection defaults and optionally import hosts from OpenSSH.</p>
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
                    aria-invalid={portInvalid || undefined}
                    aria-describedby={portInvalid ? "ob-port-error" : undefined}
                    bind:value={default_port}
                  />
                </div>
              </div>
              {#if portInvalid}
                <span id="ob-port-error" class="field-error -mt-2" role="alert">Port must be between 1 and 65535.</span>
              {/if}
              <label for="ob-agent" class="flex cursor-pointer items-center justify-between gap-4">
                <span class="flex flex-col gap-0.5">
                  <span class="text-sm text-primary">Start the SSH agent on launch</span>
                  <span class="text-xs text-muted">Keeps added keys available to every session.</span>
                </span>
                <input id="ob-agent" type="checkbox" class="switch" bind:checked={auto_start_agent} />
              </label>
              <label for="ob-fuzzy" class="flex cursor-pointer items-center justify-between gap-4">
                <span class="flex flex-col gap-0.5">
                  <span class="text-sm text-primary">Fuzzy host search</span>
                  <span class="text-xs text-muted">Match typos and partial host names.</span>
                </span>
                <input id="ob-fuzzy" type="checkbox" class="switch" bind:checked={fuzzy_search} />
              </label>
              <div class="flex flex-col gap-3 border-t border-border-subtle pt-4">
                <label for="ob-import" class="flex cursor-pointer items-center justify-between gap-4">
                  <span class="flex flex-col gap-0.5">
                    <span class="text-sm text-primary">Import from OpenSSH config</span>
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
            </div>
          {:else}
            <div class="flex flex-col gap-1">
              <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Appearance</h1>
              <p class="m-0 text-sm text-muted">You can change the theme later in Settings.</p>
            </div>
            <div
              class="grid grid-cols-1 gap-3 sm:grid-cols-3"
              role="radiogroup"
              tabindex="-1"
              aria-label="Theme"
              onkeydown={(e) => {
                const idx = THEME_CARDS.findIndex((t) => t.id === theme);
                let next = -1;
                if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (idx + 1) % THEME_CARDS.length;
                else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (idx - 1 + THEME_CARDS.length) % THEME_CARDS.length;
                if (next === -1) return;
                e.preventDefault();
                pickTheme(THEME_CARDS[next].id);
                const buttons = (e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('[role="radio"]');
                buttons[next]?.focus();
              }}
            >
              {#each THEME_CARDS as t (t.id)}
                {@const active = theme === t.id}
                <button
                  type="button"
                  role="radio"
                  aria-checked={active}
                  tabindex={active ? 0 : -1}
                  class="card card-interactive flex flex-col gap-2.5 p-2 text-left {active ? 'card-selected' : ''}"
                  onclick={() => pickTheme(t.id)}
                  data-autofocus={active ? true : undefined}
                >
                  {#if t.id === "system"}
                    <span
                      class="flex h-16 overflow-hidden rounded-md border border-border"
                      aria-hidden="true"
                    >
                      <span class="flex flex-1 overflow-hidden" style="background: {swatches.dark.surface};">
                        <span class="flex w-3.5 shrink-0 flex-col items-center gap-1 pt-2" style="background: {swatches.dark.chrome}; border-right: 1px solid {swatches.dark.border};">
                          <span class="size-1 rounded-full" style="background: {swatches.dark.accent};"></span>
                          <span class="size-1 rounded-full" style="background: {swatches.dark.muted};"></span>
                        </span>
                        <span class="flex flex-1 flex-col gap-1 p-1.5">
                          <span class="flex flex-col gap-0.5 rounded-xs p-1" style="background: {swatches.dark.panel}; border: 1px solid {swatches.dark.border};">
                            <span class="h-1 w-3/4 rounded-full" style="background: {swatches.dark.text};"></span>
                            <span class="h-1 w-1/2 rounded-full" style="background: {swatches.dark.muted};"></span>
                          </span>
                          <span class="h-1.5 w-4 rounded-xs" style="background: {swatches.dark.accent};"></span>
                        </span>
                      </span>
                      <span class="flex flex-1 overflow-hidden" style="background: {swatches.light.surface}; border-left: 1px solid {swatches.light.border};">
                        <span class="flex flex-1 flex-col gap-1 p-1.5">
                          <span class="flex flex-col gap-0.5 rounded-xs p-1" style="background: {swatches.light.panel}; border: 1px solid {swatches.light.border};">
                            <span class="h-1 w-3/4 rounded-full" style="background: {swatches.light.text};"></span>
                            <span class="h-1 w-1/2 rounded-full" style="background: {swatches.light.muted};"></span>
                          </span>
                          <span class="h-1.5 w-4 rounded-xs" style="background: {swatches.light.accent};"></span>
                        </span>
                        <span class="flex w-3.5 shrink-0 flex-col items-center gap-1 pt-2" style="background: {swatches.light.chrome}; border-left: 1px solid {swatches.light.border};">
                          <span class="size-1 rounded-full" style="background: {swatches.light.accent};"></span>
                          <span class="size-1 rounded-full" style="background: {swatches.light.muted};"></span>
                        </span>
                      </span>
                    </span>
                  {:else}
                    <span
                      class="flex h-16 overflow-hidden rounded-md border"
                      style="background: {t.surface}; border-color: {t.border};"
                      aria-hidden="true"
                    >
                      <span class="flex w-5 shrink-0 flex-col items-center gap-1 pt-2" style="background: {t.chrome}; border-right: 1px solid {t.border};">
                        <span class="size-1.5 rounded-full" style="background: {t.accent};"></span>
                        <span class="size-1.5 rounded-full" style="background: {t.muted};"></span>
                        <span class="size-1.5 rounded-full" style="background: {t.muted};"></span>
                      </span>
                      <span class="flex flex-1 flex-col gap-1.5 p-2">
                        <span class="flex flex-col gap-1 rounded-xs p-1.5" style="background: {t.panel}; border: 1px solid {t.border};">
                          <span class="h-1 w-3/4 rounded-full" style="background: {t.text};"></span>
                          <span class="h-1 w-1/2 rounded-full" style="background: {t.muted};"></span>
                        </span>
                        <span class="h-2 w-6 rounded-xs" style="background: {t.accent};"></span>
                      </span>
                    </span>
                  {/if}
                  <span class="flex items-center justify-between px-0.5 text-sm {active ? 'text-primary' : 'text-secondary'}">
                    {t.name}
                    {#if active}<Check size={14} class="text-accent" />{/if}
                  </span>
                </button>
              {/each}
            </div>
            <div class="rounded-md border border-border bg-surface-subtle px-3 py-2.5 text-xs leading-relaxed text-secondary">
              <p class="m-0">
                <span class="font-medium text-primary">Quick tip:</span> Select a host and press
                <kbd class="kbd">{CONNECT_KEY}</kbd> to connect, or run
                <code class="select-text">{CONNECT_CLI}</code> from any terminal.
              </p>
              {#if import_ssh_config}
                <p class="m-0 mt-1 text-muted">
                  Hosts from <code class="select-text">{ssh_config_path.trim() || "~/.ssh/config"}</code> will be imported into
                  <span class="text-secondary">{profileTrimmed}</span>.
                </p>
              {/if}
            </div>
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
          {step === 0 ? (path === "invite" ? "Continue" : "Get started") : step === LAST ? "Open Bayesian SSH" : "Continue"}
        </button>
      </div>
    </form>
    {/if}

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
