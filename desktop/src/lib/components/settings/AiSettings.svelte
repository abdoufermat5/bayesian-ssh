<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import {
    AlertTriangle,
    Check,
    Copy,
    CopyPlus,
    Download,
    Pencil,
    Plus,
    Shield,
    Trash2,
    X,
  } from "lucide-svelte";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import AiPolicyModal from "./AiPolicyModal.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/clipboard";
  import type { AiClientInstallResult, AiClientStatus, AiPolicy, AiSettingsView, AiTier } from "$lib/types";

  const TIER_LABEL: Record<AiTier, string> = {
    "in-client": "In the agent's client",
    "out-of-band": "This app or bssh ai approve",
    "two-person": "Two approvals",
  };

  const PROFILE_LABEL: Record<AiPolicy["profile"], string> = {
    observe: "Observe",
    operate: "Operate",
    oncall: "On-call",
  };

  const PROFILE_DESC: Record<AiPolicy["profile"], string> = {
    observe: "Read-only tools",
    operate: "Changes need approval",
    oncall: "Unattended with remote approval",
  };

  const TIMEOUT_PRESETS = [
    { label: "1 min", secs: 60 },
    { label: "5 min", secs: 300 },
    { label: "10 min", secs: 600 },
    { label: "15 min", secs: 900 },
    { label: "30 min", secs: 1800 },
  ];

  function fallbackPolicy(profile: AiPolicy["profile"]): AiPolicy {
    return {
      profile,
      scope: { tags: ["*"], hosts: [] },
      approval: {
        default: "out-of-band",
        in_client: [],
        two_person: [],
        two_person_hosts: [],
      },
      read_paths: [],
      write_paths: [],
      deny_paths: [],
      deny: [],
      budgets: {
        calls_per_min: profile === "oncall" ? 60 : 30,
        changes_per_run: profile === "observe" ? 0 : profile === "oncall" ? 20 : 10,
        run_ttl: profile === "oncall" ? "4h" : "2h",
        max_denials_per_min: 5,
      },
      redact: { vault_secrets: true, patterns: "default" },
    };
  }

  let view = $state.raw<AiSettingsView | null>(null);
  let loadError = $state<string | null>(null);

  // Global settings local state
  let timeoutSecs = $state(300);
  let recordOutput = $state(true);
  let trustedSources = $state<string[]>([]);
  let customSource = $state("");
  let savingGlobal = $state(false);
  let globalError = $state<string | null>(null);

  // Policy modal state
  let editor = $state<{
    originalName: string | null;
    initialName: string;
    initialPolicy: AiPolicy;
  } | null>(null);
  let savingPolicy = $state(false);
  let policyError = $state<string | null>(null);

  // Delete policy confirmation
  let deleteTarget = $state<string | null>(null);
  let deletingPolicy = $state(false);

  // Client MCP install state
  let installPolicy = $state("default");
  let installGuard = $state(true);
  let busyClient = $state<string | null>(null);
  let lastInstallReport = $state<string | null>(null);

  let copiedConfig = $state(false);

  function syncFromView(next: AiSettingsView) {
    view = next;
    timeoutSecs = next.approval_timeout_secs;
    recordOutput = next.record_output;
    trustedSources = [...(next.trusted_tag_sources ?? [])];
    const policyNames = Object.keys(next.policies);
    if (installPolicy !== "default" && !policyNames.includes(installPolicy)) {
      installPolicy = "default";
    }
  }

  onMount(async () => {
    try {
      syncFromView(await invoke<AiSettingsView>("ai_settings"));
    } catch (err) {
      loadError = String(err);
    }
  });

  const names = $derived(view ? Object.keys(view.policies).sort() : []);
  const availableSources = $derived(view?.available_sources ?? []);
  const clients = $derived<AiClientStatus[]>(view?.clients ?? []);
  const profileDefaults = $derived(view?.profile_defaults ?? {});

  const installPolicyOptions = $derived.by(() => {
    const set = new Set<string>(["default", ...names]);
    return [...set].map((n) => ({
      value: n,
      label: n === "default" && !names.includes("default") ? "default (built-in observe)" : n,
    }));
  });

  function formatDurationLabel(secs: number): string {
    return secs % 60 === 0 ? `${secs / 60} min` : `${secs} s`;
  }

  async function saveGlobal(
    nextTimeout = timeoutSecs,
    nextRecord = recordOutput,
    nextSources = trustedSources,
  ) {
    const secs = Math.floor(Number(nextTimeout));
    if (!Number.isFinite(secs) || secs < 15 || secs > 86_400) {
      globalError = "Approval timeout must be between 15 seconds and 24 hours (86400 s).";
      return;
    }
    savingGlobal = true;
    globalError = null;
    try {
      const updated = await invoke<AiSettingsView>("ai_save_global_settings", {
        approvalTimeoutSecs: secs,
        recordOutput: nextRecord,
        trustedTagSources: nextSources,
      });
      syncFromView(updated);
      notify("AI settings saved", "success");
    } catch (err) {
      globalError = String(err);
      notify(`Could not save AI settings: ${err}`, "error");
    } finally {
      savingGlobal = false;
    }
  }

  function toggleTrustedSource(sourceName: string) {
    const next = trustedSources.includes(sourceName)
      ? trustedSources.filter((s) => s !== sourceName)
      : [...trustedSources, sourceName];
    trustedSources = next;
    void saveGlobal(timeoutSecs, recordOutput, next);
  }

  function addCustomSource() {
    const trimmed = customSource.trim();
    if (!trimmed) return;
    customSource = "";
    if (trustedSources.includes(trimmed)) return;
    const next = [...trustedSources, trimmed];
    trustedSources = next;
    void saveGlobal(timeoutSecs, recordOutput, next);
  }

  function openNewPolicy(profile: AiPolicy["profile"] = "operate") {
    const base = profileDefaults[profile]
      ? {
          ...profileDefaults[profile],
          scope: { tags: ["*"], hosts: [] },
          // Store empty read_paths/deny_paths/deny so policy inherits built-ins cleanly.
          read_paths: [],
          deny_paths: [],
          deny: [],
        }
      : fallbackPolicy(profile);
    const defaultName = !names.includes("default")
      ? "default"
      : `${profile}-${names.length + 1}`;
    policyError = null;
    editor = {
      originalName: null,
      initialName: defaultName,
      initialPolicy: base,
    };
  }

  function openEditPolicy(name: string) {
    if (!view) return;
    const pol = view.policies[name];
    if (!pol) return;
    policyError = null;
    editor = {
      originalName: name,
      initialName: name,
      initialPolicy: pol,
    };
  }

  function openDuplicatePolicy(name: string) {
    if (!view) return;
    const pol = view.policies[name];
    if (!pol) return;
    let copyName = `${name}-copy`;
    let i = 2;
    while (names.includes(copyName)) {
      copyName = `${name}-copy-${i++}`;
    }
    policyError = null;
    editor = {
      originalName: null,
      initialName: copyName,
      initialPolicy: pol,
    };
  }

  async function handleSavePolicy(name: string, originalName: string | null, policy: AiPolicy) {
    savingPolicy = true;
    policyError = null;
    try {
      const updated = await invoke<AiSettingsView>("ai_save_policy", {
        name,
        originalName,
        policy,
      });
      syncFromView(updated);
      editor = null;
      notify(originalName ? `Saved policy "${name}"` : `Created policy "${name}"`, "success");
    } catch (err) {
      policyError = String(err);
    } finally {
      savingPolicy = false;
    }
  }

  async function confirmDeletePolicy() {
    if (!deleteTarget) return;
    const target = deleteTarget;
    deletingPolicy = true;
    try {
      const updated = await invoke<AiSettingsView>("ai_delete_policy", { name: target });
      syncFromView(updated);
      deleteTarget = null;
      notify(`Deleted policy "${target}"`, "success");
    } catch (err) {
      notify(`Could not delete policy: ${err}`, "error");
    } finally {
      deletingPolicy = false;
    }
  }

  async function handleClientAction(client: AiClientStatus, uninstall: boolean) {
    busyClient = client.id;
    lastInstallReport = null;
    try {
      const result = await invoke<AiClientInstallResult>("ai_install_client", {
        client: client.id,
        policyName: installPolicy,
        guard: client.id === "claude-code" && installGuard && !uninstall,
        uninstall,
      });
      syncFromView(result.settings);
      lastInstallReport = result.report.trim() || null;
      notify(
        uninstall
          ? `Removed bssh from ${client.label}`
          : `Configured ${client.label} with policy "${installPolicy}"`,
        "success",
      );
    } catch (err) {
      notify(`${client.label}: ${err}`, "error");
    } finally {
      busyClient = null;
    }
  }

  function copyConfigPath() {
    if (!view?.config_path) return;
    copyTextWithFallback(view.config_path);
    copiedConfig = true;
    notify("Copied config path", "success");
    setTimeout(() => (copiedConfig = false), 1500);
  }
</script>

<div class="settings-page">
  {#if loadError}
    <div class="alert alert-error" role="alert">
      <AlertTriangle size={15} class="mt-0.5 shrink-0" />
      <div class="alert-body">Could not read the AI settings: {loadError}</div>
    </div>
  {:else if !view}
    <div class="skeleton h-24 w-full rounded-lg" aria-busy="true" aria-label="Loading AI settings"></div>
  {:else}
    <!-- Section 1: Global approvals, recording, trusted tag sources -->
    <section>
      <h2 class="settings-group-title">Approvals &amp; recording</h2>
      {#if globalError}
        <div class="alert alert-error mb-3" role="alert">
          <AlertTriangle size={15} class="mt-0.5 shrink-0" />
          <div class="alert-body">{globalError}</div>
        </div>
      {/if}
      <div class="settings-group">
        <div class="setting-row">
          <div class="setting-row-main">
            <label for="ai-approval-timeout" class="setting-title">Unanswered requests expire after</label>
            <span class="setting-meta">
              An approval nobody answers counts as denied ({formatDurationLabel(view.approval_timeout_secs)}).
            </span>
          </div>
          <div class="setting-control flex-wrap justify-end">
            <div class="flex flex-wrap items-center gap-1" role="group" aria-label="Approval timeout presets">
              {#each TIMEOUT_PRESETS as preset (preset.secs)}
                <button
                  type="button"
                  class="btn btn-sm {timeoutSecs === preset.secs ? 'btn-secondary' : 'btn-ghost'}"
                  aria-pressed={timeoutSecs === preset.secs}
                  disabled={savingGlobal}
                  onclick={() => {
                    timeoutSecs = preset.secs;
                    void saveGlobal(preset.secs, recordOutput, trustedSources);
                  }}
                >
                  {preset.label}
                </button>
              {/each}
            </div>
            <div class="flex items-center gap-1.5">
              <input
                id="ai-approval-timeout"
                type="number"
                min="15"
                max="86400"
                step="15"
                class="input input-mono w-24 tabular-nums"
                bind:value={timeoutSecs}
                disabled={savingGlobal}
                onchange={() => void saveGlobal(timeoutSecs, recordOutput, trustedSources)}
              />
              <span class="text-xs text-muted">sec</span>
            </div>
          </div>
        </div>

        <label for="ai-record-output" class="setting-row cursor-pointer">
          <div class="setting-row-main">
            <span class="setting-title">Record agent output</span>
            <span class="setting-meta">Keep what commands print as asciicast, like interactive sessions.</span>
          </div>
          <div class="setting-control">
            <input
              id="ai-record-output"
              type="checkbox"
              class="switch"
              bind:checked={recordOutput}
              disabled={savingGlobal}
              onchange={() => void saveGlobal(timeoutSecs, recordOutput, trustedSources)}
            />
          </div>
        </label>

        <div class="setting-row items-start">
          <div class="setting-row-main">
            <label for="ai-trusted-source-input" class="setting-title">Trusted tag sources</label>
            <span class="setting-meta">
              Inventory sources whose tags may widen scope or allow in-client approval. Manual host tags are always
              trusted; untrusted sources can only tighten policy.
            </span>
            {#if availableSources.length > 0}
              <div class="mt-2 flex flex-wrap gap-1.5" role="group" aria-label="Available inventory sources">
                {#each availableSources as src (src)}
                  {@const active = trustedSources.includes(src)}
                  <button
                    type="button"
                    class="chip {active ? 'chip-active' : ''}"
                    aria-pressed={active}
                    disabled={savingGlobal}
                    onclick={() => toggleTrustedSource(src)}
                  >
                    {src}
                  </button>
                {/each}
              </div>
            {/if}
            {#if trustedSources.some((s) => !availableSources.includes(s))}
              <div class="mt-2 flex flex-wrap gap-1.5">
                {#each trustedSources.filter((s) => !availableSources.includes(s)) as src (src)}
                  <span class="tag gap-1 pr-0.5">
                    {src}
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                      disabled={savingGlobal}
                      onclick={() => toggleTrustedSource(src)}
                      aria-label="Remove trusted source {src}"
                    >
                      <X size={11} />
                    </button>
                  </span>
                {/each}
              </div>
            {/if}
          </div>
          <div class="setting-control">
            <input
              id="ai-trusted-source-input"
              type="text"
              class="input input-mono w-40"
              placeholder="Source name"
              autocomplete="off"
              spellcheck="false"
              bind:value={customSource}
              disabled={savingGlobal}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addCustomSource();
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary btn-sm"
              disabled={savingGlobal || !customSource.trim()}
              onclick={addCustomSource}
            >
              Add
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- Section 2: Named Policies -->
    <section>
      <div class="mb-2 flex items-center justify-between gap-2">
        <h2 class="settings-group-title m-0">Policies</h2>
        <button type="button" class="btn btn-primary btn-sm" onclick={() => openNewPolicy("operate")}>
          <Plus size={14} />
          New policy
        </button>
      </div>

      {#if names.length === 0}
        <div class="settings-group">
          <div class="empty-state py-10">
            <div class="empty-state-icon"><Shield size={18} /></div>
            <div class="empty-state-title">No custom policy defined</div>
            <p class="empty-state-desc">
              Agents currently run with the built-in read-only Observe profile across all hosts. Create a policy to
              scope hosts, enable approvals, or tune budgets.
            </p>
            <div class="empty-state-action">
              <button type="button" class="btn btn-secondary" onclick={() => openNewPolicy("observe")}>
                Start with Observe
              </button>
              <button type="button" class="btn btn-primary" onclick={() => openNewPolicy("operate")}>
                <Plus size={14} />
                Create Operate policy
              </button>
            </div>
          </div>
        </div>
      {:else}
        <div class="flex flex-col gap-3">
          {#each names as name (name)}
            {@const p = view.policies[name]}
            {@const policyWarnings = view.warnings?.[name] ?? []}
            {@const isAllHosts = p.scope.tags.includes("*")}
            <section class="panel overflow-hidden" aria-label="Policy {name}">
              <div class="panel-header">
                <div class="flex min-w-0 flex-wrap items-center gap-2">
                  <h3 class="panel-title font-mono">{name}</h3>
                  <span class="badge badge-neutral">
                    {PROFILE_LABEL[p.profile] ?? p.profile} · {PROFILE_DESC[p.profile] ?? ""}
                  </span>
                  {#if name === "default"}
                    <span class="badge badge-accent">Default</span>
                  {/if}
                </div>
                <div class="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    onclick={() => openEditPolicy(name)}
                    aria-label="Edit policy {name}"
                  >
                    <Pencil size={13} />
                    Edit
                  </button>
                  <button
                    type="button"
                    class="btn-icon"
                    onclick={() => openDuplicatePolicy(name)}
                    aria-label="Duplicate policy {name}"
                    title="Duplicate policy"
                  >
                    <CopyPlus size={14} />
                  </button>
                  <button
                    type="button"
                    class="btn-icon btn-icon-danger"
                    onclick={() => (deleteTarget = name)}
                    aria-label="Delete policy {name}"
                    title="Delete policy"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>

              {#if policyWarnings.length > 0}
                <div class="border-b border-border px-4 py-2.5">
                  <div class="alert alert-warning" role="alert">
                    <AlertTriangle size={14} class="mt-0.5 shrink-0" />
                    <div class="alert-body">
                      {#each policyWarnings as w (w)}
                        <span>{w}</span>
                      {/each}
                    </div>
                  </div>
                </div>
              {/if}

              <div class="grid grid-cols-1 gap-x-6 gap-y-3 p-4 text-xs sm:grid-cols-2">
                <div class="flex flex-col gap-1">
                  <span class="eyebrow">Scope</span>
                  <div class="flex flex-wrap items-center gap-1">
                    {#if isAllHosts}
                      <span class="tag">All hosts (*)</span>
                    {:else if p.scope.tags.length === 0 && p.scope.hosts.length === 0}
                      <span class="text-warning">No hosts in scope</span>
                    {:else}
                      {#each p.scope.tags as t (t)}<span class="tag">#{t}</span>{/each}
                      {#each p.scope.hosts as h (h)}<span class="tag">{h}</span>{/each}
                    {/if}
                  </div>
                </div>

                <div class="flex flex-col gap-1">
                  <span class="eyebrow">Approvals</span>
                  <span class="text-primary">{TIER_LABEL[p.approval.default] ?? p.approval.default}</span>
                  {#if p.approval.in_client.length > 0 || p.approval.two_person.length > 0 || p.approval.two_person_hosts.length > 0}
                    <div class="flex flex-wrap gap-1 pt-0.5">
                      {#each p.approval.in_client as t (t)}<span class="tag">in-client: {t}</span>{/each}
                      {#each p.approval.two_person as t (t)}<span class="tag">2-person: {t}</span>{/each}
                      {#each p.approval.two_person_hosts as h (h)}<span class="tag">2-person host: {h}</span>{/each}
                    </div>
                  {/if}
                </div>

                <div class="flex flex-col gap-1">
                  <span class="eyebrow">Paths &amp; commands</span>
                  <div class="flex flex-wrap gap-1">
                    {#if p.read_paths.length > 0}
                      <span class="tag">read: {p.read_paths.length}</span>
                    {:else}
                      <span class="tag">read: defaults</span>
                    {/if}
                    {#if p.write_paths.length > 0}
                      <span class="tag">write: {p.write_paths.length}</span>
                    {/if}
                    {#if p.deny_paths.length > 0}
                      <span class="tag">+{p.deny_paths.length} deny paths</span>
                    {/if}
                    {#if p.deny.length > 0}
                      <span class="tag">+{p.deny.length} blocked cmds</span>
                    {/if}
                  </div>
                  {#if p.deny_paths.length > 0 || p.deny.length > 0}
                    <div class="flex flex-wrap gap-x-2.5 gap-y-0.5 pt-0.5">
                      {#each p.deny_paths.slice(0, 3) as dp (dp)}<code class="mono text-muted">{dp}</code>{/each}
                      {#each p.deny.slice(0, 3) as cmd (cmd)}<code class="mono text-muted">{cmd}</code>{/each}
                    </div>
                  {/if}
                </div>

                <div class="flex flex-col gap-1">
                  <span class="eyebrow">Budgets &amp; redaction</span>
                  <span class="mono tabular-nums text-secondary">
                    {p.budgets.calls_per_min}/min · {p.budgets.changes_per_run} changes · TTL {p.budgets.run_ttl} ·
                    breaker {p.budgets.max_denials_per_min}/min
                  </span>
                  <span class="text-muted">
                    Redaction: {p.redact.vault_secrets ? "vault + " : ""}patterns {p.redact.patterns}
                  </span>
                </div>
              </div>
            </section>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Section 3: Connected AI Clients (MCP Setup) -->
    {#if clients.length > 0}
      <section>
        <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
          <h2 class="settings-group-title m-0">Connected AI clients (MCP)</h2>
          <div class="flex items-center gap-2">
            <label for="ai-install-policy" class="text-xs text-muted">Install with policy</label>
            <div class="w-48">
              <CustomSelect
                id="ai-install-policy"
                ariaLabel="Policy for client install"
                size="sm"
                options={installPolicyOptions}
                bind:value={installPolicy}
              />
            </div>
          </div>
        </div>

        <div class="settings-group">
          <label for="ai-install-guard" class="setting-row cursor-pointer">
            <div class="setting-row-main">
              <span class="setting-title">Install Claude Code PreToolUse guard hook</span>
              <span class="setting-meta">
                Intercepts direct <code class="mono">ssh</code> commands in Claude Code so they route through bssh
                policy checks.
              </span>
            </div>
            <div class="setting-control">
              <input id="ai-install-guard" type="checkbox" class="switch" bind:checked={installGuard} />
            </div>
          </label>

          {#each clients as client (client.id)}
            {@const isBusy = busyClient === client.id}
            <div class="setting-row">
              <div class="setting-row-main">
                <div class="flex items-center gap-2">
                  <span class="setting-title font-medium">{client.label}</span>
                  {#if client.configured}
                    <span class="badge badge-success">Configured</span>
                  {:else if client.present}
                    <span class="badge badge-neutral">Detected</span>
                  {:else}
                    <span class="badge badge-neutral text-muted">Not detected</span>
                  {/if}
                </div>
                <span class="setting-meta truncate font-mono" title={client.config_path}>{client.config_path}</span>
              </div>
              <div class="setting-control">
                {#if client.configured}
                  <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    disabled={busyClient !== null}
                    onclick={() => void handleClientAction(client, false)}
                  >
                    {#if isBusy}<span class="spinner size-3" aria-hidden="true"></span>{/if}
                    Update
                  </button>
                  <button
                    type="button"
                    class="btn btn-danger-ghost btn-sm"
                    disabled={busyClient !== null}
                    onclick={() => void handleClientAction(client, true)}
                  >
                    Uninstall
                  </button>
                {:else}
                  <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    disabled={busyClient !== null}
                    onclick={() => void handleClientAction(client, false)}
                  >
                    {#if isBusy}
                      <span class="spinner size-3" aria-hidden="true"></span>
                    {:else}
                      <Download size={13} />
                    {/if}
                    Install
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </div>

        {#if lastInstallReport}
          <pre class="code-block mt-3 max-h-40 whitespace-pre-wrap">{lastInstallReport}</pre>
        {/if}
      </section>
    {/if}

    <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
      <span>
        Saved in <code class="mono text-secondary">{view.config_path}</code>. Restart running AI agents to apply
        policy changes.
      </span>
      <button
        type="button"
        class="btn btn-ghost btn-sm"
        onclick={copyConfigPath}
        aria-label="Copy config path"
      >
        {#if copiedConfig}<Check size={13} class="text-success" />{:else}<Copy size={13} />{/if}
        Copy path
      </button>
    </div>
  {/if}
</div>

{#if editor}
  <AiPolicyModal
    originalName={editor.originalName}
    initialName={editor.initialName}
    initialPolicy={editor.initialPolicy}
    {profileDefaults}
    existingNames={names}
    saving={savingPolicy}
    error={policyError}
    onClose={() => !savingPolicy && (editor = null)}
    onSave={handleSavePolicy}
  />
{/if}

{#if deleteTarget}
  <ModalShell
    open={true}
    title="Delete policy"
    role="alertdialog"
    width="sm"
    closeOnEscape={!deletingPolicy}
    closeOnBackdrop={!deletingPolicy}
    onClose={() => !deletingPolicy && (deleteTarget = null)}
  >
    <div class="modal-header">
      <h2 class="modal-title">Delete policy "{deleteTarget}"?</h2>
      <button
        type="button"
        class="modal-close"
        disabled={deletingPolicy}
        onclick={() => (deleteTarget = null)}
        aria-label="Close"
      >
        <X size={16} />
      </button>
    </div>
    <div class="modal-body">
      <p class="m-0 text-sm text-secondary">
        This removes <code class="mono text-primary">{deleteTarget}</code> from your environment's
        <code class="mono">config.json</code>.
        {#if deleteTarget === "default"}
          Without a custom <code class="mono">default</code> policy, agents fall back to the built-in read-only Observe
          profile.
        {/if}
      </p>
    </div>
    <div class="modal-footer">
      <button
        type="button"
        class="btn btn-secondary"
        disabled={deletingPolicy}
        onclick={() => (deleteTarget = null)}
        data-autofocus
      >
        Cancel
      </button>
      <button type="button" class="btn btn-danger" disabled={deletingPolicy} onclick={confirmDeletePolicy}>
        {#if deletingPolicy}<span class="spinner size-3" aria-hidden="true"></span>{/if}
        Delete policy
      </button>
    </div>
  </ModalShell>
{/if}
