<script lang="ts">
  import { untrack } from "svelte";
  import { AlertTriangle, Check, ChevronDown, ChevronRight, Plus, RotateCcw, Shield, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { appState } from "$lib/stores/appState.svelte";
  import type { AiPolicy, AiTier } from "$lib/types";

  interface Props {
    /** Existing policy name when editing, or null when creating a new policy. */
    originalName: string | null;
    initialName: string;
    initialPolicy: AiPolicy;
    /** Built-in profile defaults returned by `ai_settings()` (for presets and built-in guardrail previews). */
    profileDefaults: Record<string, AiPolicy>;
    existingNames: string[];
    saving: boolean;
    error: string | null;
    onClose: () => void;
    onSave: (name: string, originalName: string | null, policy: AiPolicy) => void | Promise<void>;
  }

  let {
    originalName,
    initialName,
    initialPolicy,
    profileDefaults,
    existingNames,
    saving,
    error,
    onClose,
    onSave,
  }: Props = $props();

  type TabId = "general" | "approvals" | "guardrails" | "budgets";

  const PROFILES: Array<{
    id: AiPolicy["profile"];
    label: string;
    summary: string;
  }> = [
    {
      id: "observe",
      label: "Observe",
      summary: "Read-only inspection. All write/edit tools are blocked.",
    },
    {
      id: "operate",
      label: "Operate",
      summary: "Reads allowed; changes require human approval.",
    },
    {
      id: "oncall",
      label: "On-call",
      summary: "Higher budgets and longer sessions with out-of-band approval.",
    },
  ];

  const TIER_OPTIONS: Array<{ value: AiTier; label: string; description: string }> = [
    {
      value: "out-of-band",
      label: "This app or bssh ai approve",
      description: "Prompt outside the agent client (recommended)",
    },
    {
      value: "in-client",
      label: "In the agent's client",
      description: "Confirm low-risk reversible actions inside MCP client",
    },
    {
      value: "two-person",
      label: "Two approvals",
      description: "Require two separate human approvals (Teams)",
    },
  ];

  const REDACT_OPTIONS = [
    { value: "default", label: "Default patterns", description: "Mask keys, tokens, passwords and headers" },
    { value: "off", label: "Off", description: "Do not scan output for secret patterns" },
  ];

  const TTL_PRESETS = ["30m", "1h", "2h", "4h", "8h", "24h"];

  function clonePolicy(p: AiPolicy): AiPolicy {
    return {
      profile: p.profile,
      scope: { tags: [...p.scope.tags], hosts: [...p.scope.hosts] },
      approval: {
        default: p.approval.default,
        in_client: [...p.approval.in_client],
        two_person: [...p.approval.two_person],
        two_person_hosts: [...p.approval.two_person_hosts],
      },
      read_paths: [...p.read_paths],
      write_paths: [...p.write_paths],
      deny_paths: [...p.deny_paths],
      deny: [...p.deny],
      budgets: { ...p.budgets },
      redact: { ...p.redact },
    };
  }

  let activeTab = $state<TabId>("general");
  let name = $state(untrack(() => initialName));
  let draft = $state<AiPolicy>(untrack(() => clonePolicy(initialPolicy)));
  let allHosts = $state(untrack(() => initialPolicy.scope.tags.includes("*")));
  let attempted = $state(false);
  let showBuiltins = $state(false);

  // Chip inputs
  let tagInput = $state("");
  let hostInput = $state("");
  let inClientInput = $state("");
  let twoPersonInput = $state("");
  let twoPersonHostInput = $state("");
  let readPathInput = $state("");
  let writePathInput = $state("");
  let denyPathInput = $state("");
  let denyCmdInput = $state("");

  const isEditing = $derived(originalName !== null);
  const title = $derived(isEditing ? `Edit policy: ${originalName}` : "New AI policy");

  const builtInForProfile = $derived(profileDefaults[draft.profile] ?? null);
  const builtInDenyPaths = $derived(builtInForProfile?.deny_paths ?? []);
  const builtInDenyCmds = $derived(builtInForProfile?.deny ?? []);
  const builtInReadPaths = $derived(builtInForProfile?.read_paths ?? []);

  // Exclude built-in deny entries when displaying user-added extras if desired,
  // while preserving whatever the user actually stored in config.json.
  const scopeTags = $derived(draft.scope.tags.filter((t) => t !== "*"));

  const suggestedTags = $derived(
    appState.allTags.filter((t) => !scopeTags.includes(t)).slice(0, 8),
  );
  const suggestedHosts = $derived(
    appState.allConnections
      .map((c) => c.name)
      .filter((h) => !draft.scope.hosts.includes(h))
      .slice(0, 6),
  );

  function parseDurationValid(raw: string): boolean {
    const s = raw.trim();
    if (!s) return false;
    if (/^\d+$/.test(s)) return Number(s) > 0;
    const parts = s.match(/^(\d+[smhd])+$/);
    if (!parts) return false;
    let total = 0;
    for (const m of s.matchAll(/(\d+)([smhd])/g)) {
      const n = Number(m[1]);
      const mult = m[2] === "s" ? 1 : m[2] === "m" ? 60 : m[2] === "h" ? 3600 : 86400;
      total += n * mult;
    }
    return total > 0;
  }

  const trimmedName = $derived(name.trim());
  const nameError = $derived.by(() => {
    if (!attempted && !trimmedName) return null;
    if (!trimmedName) return "Give the policy a name (for example, default).";
    if (trimmedName.length > 64) return "Policy name must be 64 characters or fewer.";
    if (!/^[A-Za-z0-9._-]+$/.test(trimmedName)) {
      return "Use only letters, digits, '-', '_', and '.'.";
    }
    if (trimmedName !== originalName && existingNames.includes(trimmedName)) {
      return `A policy named "${trimmedName}" already exists.`;
    }
    return null;
  });

  const scopeEmpty = $derived(!allHosts && scopeTags.length === 0 && draft.scope.hosts.length === 0);
  const scopeError = $derived(
    attempted && scopeEmpty
      ? "Enable All hosts (*) or add at least one tag or host so this policy can match hosts."
      : null,
  );

  const ttlError = $derived(
    (attempted || draft.budgets.run_ttl.trim().length > 0) && !parseDurationValid(draft.budgets.run_ttl)
      ? "Use durations like 30m, 2h, 4h, or 1d."
      : null,
  );

  function addItem(list: string[], raw: string): string[] {
    const parts = raw
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    if (parts.length === 0) return list;
    const next = [...list];
    for (const p of parts) {
      if (!next.includes(p)) next.push(p);
    }
    return next;
  }

  function removeItem(list: string[], value: string): string[] {
    return list.filter((x) => x !== value);
  }

  function selectProfile(profile: AiPolicy["profile"]) {
    draft.profile = profile;
    const preset = profileDefaults[profile];
    if (preset && !isEditing) {
      draft.budgets = { ...preset.budgets };
    }
  }

  function applyProfileDefaults() {
    const preset = profileDefaults[draft.profile];
    if (!preset) return;
    draft.budgets = { ...preset.budgets };
    draft.read_paths = [...preset.read_paths];
    draft.approval = { ...draft.approval, default: preset.approval.default };
    draft.redact = { ...preset.redact };
  }

  function addScopeTag(val?: string) {
    const text = (val ?? tagInput).trim();
    if (!text) return;
    if (text === "*") {
      allHosts = true;
    } else {
      draft.scope.tags = addItem(scopeTags, text);
    }
    if (val === undefined) tagInput = "";
  }

  function addScopeHost(val?: string) {
    const text = (val ?? hostInput).trim();
    if (!text) return;
    draft.scope.hosts = addItem(draft.scope.hosts, text);
    if (val === undefined) hostInput = "";
  }

  function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    attempted = true;

    // Flush any pending text sitting in chip inputs before validating.
    if (tagInput.trim()) addScopeTag();
    if (hostInput.trim()) addScopeHost();
    if (inClientInput.trim()) {
      draft.approval.in_client = addItem(draft.approval.in_client, inClientInput);
      inClientInput = "";
    }
    if (twoPersonInput.trim()) {
      draft.approval.two_person = addItem(draft.approval.two_person, twoPersonInput);
      twoPersonInput = "";
    }
    if (twoPersonHostInput.trim()) {
      draft.approval.two_person_hosts = addItem(draft.approval.two_person_hosts, twoPersonHostInput);
      twoPersonHostInput = "";
    }
    if (readPathInput.trim()) {
      draft.read_paths = addItem(draft.read_paths, readPathInput);
      readPathInput = "";
    }
    if (writePathInput.trim()) {
      draft.write_paths = addItem(draft.write_paths, writePathInput);
      writePathInput = "";
    }
    if (denyPathInput.trim()) {
      draft.deny_paths = addItem(draft.deny_paths, denyPathInput);
      denyPathInput = "";
    }
    if (denyCmdInput.trim()) {
      draft.deny = addItem(draft.deny, denyCmdInput);
      denyCmdInput = "";
    }

    if (nameError || scopeEmpty) {
      activeTab = "general";
      return;
    }
    if (!parseDurationValid(draft.budgets.run_ttl)) {
      activeTab = "budgets";
      return;
    }

    const payload: AiPolicy = {
      ...clonePolicy(draft),
      scope: {
        tags: allHosts ? ["*"] : scopeTags,
        hosts: allHosts ? [] : [...draft.scope.hosts],
      },
      budgets: {
        calls_per_min: Math.max(0, Math.floor(Number(draft.budgets.calls_per_min) || 0)),
        changes_per_run: Math.max(0, Math.floor(Number(draft.budgets.changes_per_run) || 0)),
        run_ttl: draft.budgets.run_ttl.trim(),
        max_denials_per_min: Math.max(0, Math.floor(Number(draft.budgets.max_denials_per_min) || 0)),
      },
    };
    void onSave(trimmedName, originalName, payload);
  }
</script>

<ModalShell open={true} {title} {onClose} width="lg" closeOnEscape={!saving} closeOnBackdrop={!saving}>
  <form class="flex min-h-0 flex-1 flex-col" novalidate onsubmit={handleSubmit}>
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">{title}</h2>
        <p class="modal-subtitle">
          Saved to your environment's <code class="mono">config.json</code>. Restart running AI agents to apply changes.
        </p>
      </div>
      <button type="button" class="modal-close" onclick={onClose} disabled={saving} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="tabs shrink-0 px-5" role="tablist" aria-label="Policy sections">
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === "general"}
        class="tab {activeTab === 'general' ? 'tab-active' : ''}"
        onclick={() => (activeTab = "general")}
      >
        General &amp; scope
        {#if attempted && (nameError || scopeEmpty)}
          <span class="status-dot status-dot-sm status-dot-error" aria-hidden="true"></span>
        {/if}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === "approvals"}
        class="tab {activeTab === 'approvals' ? 'tab-active' : ''}"
        onclick={() => (activeTab = "approvals")}
      >
        Approvals
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === "guardrails"}
        class="tab {activeTab === 'guardrails' ? 'tab-active' : ''}"
        onclick={() => (activeTab = "guardrails")}
      >
        Paths &amp; commands
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === "budgets"}
        class="tab {activeTab === 'budgets' ? 'tab-active' : ''}"
        onclick={() => (activeTab = "budgets")}
      >
        Budgets &amp; safety
        {#if ttlError}
          <span class="status-dot status-dot-sm status-dot-error" aria-hidden="true"></span>
        {/if}
      </button>
    </div>

    <div class="modal-body flex flex-col gap-5 pt-4">
      {#if error}
        <div class="alert alert-error" role="alert">
          <AlertTriangle size={15} class="mt-0.5 shrink-0" />
          <div class="alert-body">{error}</div>
        </div>
      {/if}

      {#if activeTab === "general"}
        <div class="field">
          <label for="ai-policy-name" class="field-label">Policy name</label>
          <input
            id="ai-policy-name"
            type="text"
            class="input input-mono max-w-xs"
            class:input-invalid={Boolean(nameError)}
            placeholder="default"
            autocomplete="off"
            spellcheck="false"
            bind:value={name}
            disabled={saving}
            data-autofocus
          />
          {#if nameError}
            <span class="field-error">{nameError}</span>
          {:else}
            <span class="field-hint">
              Named <code class="mono">default</code> applies automatically unless an agent passes
              <code class="mono">--policy &lt;name&gt;</code>.
            </span>
          {/if}
        </div>

        <div class="field">
          <div class="flex items-center justify-between gap-2">
            <span class="field-label">Profile</span>
            {#if builtInForProfile}
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                onclick={applyProfileDefaults}
                title="Reset budgets, read paths, and approval defaults to this profile's baseline"
              >
                <RotateCcw size={12} />
                Apply {draft.profile} defaults
              </button>
            {/if}
          </div>
          <div class="grid grid-cols-1 gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Policy profile">
            {#each PROFILES as opt (opt.id)}
              {@const selected = draft.profile === opt.id}
              <button
                type="button"
                role="radio"
                aria-checked={selected}
                class="card card-interactive flex flex-col gap-1 p-3 text-left {selected ? 'card-selected' : ''}"
                onclick={() => selectProfile(opt.id)}
              >
                <span class="flex items-center justify-between gap-2">
                  <span class="text-sm font-medium text-primary">{opt.label}</span>
                  {#if selected}
                    <Check size={14} class="shrink-0 text-accent" />
                  {/if}
                </span>
                <span class="text-xs leading-snug text-muted">{opt.summary}</span>
              </button>
            {/each}
          </div>
        </div>

        <div class="flex flex-col gap-3 rounded-lg border border-border bg-panel p-4">
          <label for="ai-scope-all" class="flex cursor-pointer items-center justify-between gap-4">
            <span class="flex min-w-0 flex-col gap-0.5">
              <span class="text-sm font-medium text-primary">Allow all hosts (<code class="mono">*</code>)</span>
              <span class="text-xs text-muted">
                Include every host in this profile. Turn off to restrict the agent to specific tags or hosts.
              </span>
            </span>
            <input id="ai-scope-all" type="checkbox" class="switch" bind:checked={allHosts} />
          </label>

          {#if !allHosts}
            <div class="divider"></div>
            <div class="field">
              <label for="ai-scope-tags" class="field-label">Allowed host tags</label>
              <div class="flex gap-2">
                <input
                  id="ai-scope-tags"
                  type="text"
                  class="input input-mono flex-1"
                  placeholder="staging, dev"
                  autocomplete="off"
                  spellcheck="false"
                  bind:value={tagInput}
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addScopeTag();
                    }
                  }}
                />
                <button type="button" class="btn btn-secondary" onclick={() => addScopeTag()}>
                  <Plus size={14} />
                  Add
                </button>
              </div>
              {#if scopeTags.length > 0}
                <div class="flex flex-wrap gap-1.5 pt-0.5">
                  {#each scopeTags as tag (tag)}
                    <span class="tag gap-1 pr-0.5">
                      {tag}
                      <button
                        type="button"
                        class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                        onclick={() => (draft.scope.tags = removeItem(scopeTags, tag))}
                        aria-label="Remove tag {tag}"
                      >
                        <X size={11} />
                      </button>
                    </span>
                  {/each}
                </div>
              {/if}
              {#if suggestedTags.length > 0}
                <div class="flex flex-wrap items-center gap-1 pt-0.5">
                  <span class="text-2xs text-muted">Available tags:</span>
                  {#each suggestedTags as tag (tag)}
                    <button
                      type="button"
                      class="rounded-sm border border-border bg-surface-subtle px-1.5 py-0.5 font-mono text-2xs text-secondary transition-colors hover:border-border-hover hover:text-primary"
                      onclick={() => addScopeTag(tag)}
                    >
                      + {tag}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>

            <div class="field">
              <label for="ai-scope-hosts" class="field-label">Allowed specific hosts</label>
              <div class="flex gap-2">
                <input
                  id="ai-scope-hosts"
                  type="text"
                  class="input input-mono flex-1"
                  placeholder="prod-api-01, bastion"
                  autocomplete="off"
                  spellcheck="false"
                  bind:value={hostInput}
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addScopeHost();
                    }
                  }}
                />
                <button type="button" class="btn btn-secondary" onclick={() => addScopeHost()}>
                  <Plus size={14} />
                  Add
                </button>
              </div>
              {#if draft.scope.hosts.length > 0}
                <div class="flex flex-wrap gap-1.5 pt-0.5">
                  {#each draft.scope.hosts as h (h)}
                    <span class="tag gap-1 pr-0.5">
                      {h}
                      <button
                        type="button"
                        class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                        onclick={() => (draft.scope.hosts = removeItem(draft.scope.hosts, h))}
                        aria-label="Remove host {h}"
                      >
                        <X size={11} />
                      </button>
                    </span>
                  {/each}
                </div>
              {/if}
              {#if suggestedHosts.length > 0}
                <div class="flex flex-wrap items-center gap-1 pt-0.5">
                  <span class="text-2xs text-muted">Saved hosts:</span>
                  {#each suggestedHosts as h (h)}
                    <button
                      type="button"
                      class="rounded-sm border border-border bg-surface-subtle px-1.5 py-0.5 font-mono text-2xs text-secondary transition-colors hover:border-border-hover hover:text-primary"
                      onclick={() => addScopeHost(h)}
                    >
                      + {h}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}

          {#if scopeError}
            <span class="field-error">{scopeError}</span>
          {/if}
        </div>
      {:else if activeTab === "approvals"}
        <div class="field">
          <label for="ai-approval-default" class="field-label">Default approval channel</label>
          <CustomSelect
            id="ai-approval-default"
            ariaLabel="Default approval channel"
            options={TIER_OPTIONS}
            bind:value={draft.approval.default}
          />
          <span class="field-hint">
            Used whenever a tool call requires confirmation and no tag or host override matches below.
          </span>
        </div>

        <div class="field">
          <label for="ai-approval-in-client" class="field-label">Approve in-client tags (T1)</label>
          <div class="flex gap-2">
            <input
              id="ai-approval-in-client"
              type="text"
              class="input input-mono flex-1"
              placeholder="staging, dev"
              autocomplete="off"
              spellcheck="false"
              bind:value={inClientInput}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  draft.approval.in_client = addItem(draft.approval.in_client, inClientInput);
                  inClientInput = "";
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => {
                draft.approval.in_client = addItem(draft.approval.in_client, inClientInput);
                inClientInput = "";
              }}
            >
              <Plus size={14} />
              Add
            </button>
          </div>
          <span class="field-hint">
            Low-risk, reversible changes on hosts with these trusted tags may be approved directly in the agent client.
          </span>
          {#if draft.approval.in_client.length > 0}
            <div class="flex flex-wrap gap-1.5">
              {#each draft.approval.in_client as tag (tag)}
                <span class="tag gap-1 pr-0.5">
                  {tag}
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                    onclick={() => (draft.approval.in_client = removeItem(draft.approval.in_client, tag))}
                    aria-label="Remove in-client tag {tag}"
                  >
                    <X size={11} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
        </div>

        <div class="field">
          <label for="ai-approval-two-person" class="field-label">Two-person approval tags (T3)</label>
          <div class="flex gap-2">
            <input
              id="ai-approval-two-person"
              type="text"
              class="input input-mono flex-1"
              placeholder="prod, prod-db"
              autocomplete="off"
              spellcheck="false"
              bind:value={twoPersonInput}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  draft.approval.two_person = addItem(draft.approval.two_person, twoPersonInput);
                  twoPersonInput = "";
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => {
                draft.approval.two_person = addItem(draft.approval.two_person, twoPersonInput);
                twoPersonInput = "";
              }}
            >
              <Plus size={14} />
              Add
            </button>
          </div>
          <span class="field-hint">Requires two separate approvals for hosts carrying any of these tags.</span>
          {#if draft.approval.two_person.length > 0}
            <div class="flex flex-wrap gap-1.5">
              {#each draft.approval.two_person as tag (tag)}
                <span class="tag gap-1 pr-0.5">
                  {tag}
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                    onclick={() => (draft.approval.two_person = removeItem(draft.approval.two_person, tag))}
                    aria-label="Remove two-person tag {tag}"
                  >
                    <X size={11} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
        </div>

        <div class="field">
          <label for="ai-approval-two-person-hosts" class="field-label">Two-person approval host globs</label>
          <div class="flex gap-2">
            <input
              id="ai-approval-two-person-hosts"
              type="text"
              class="input input-mono flex-1"
              placeholder="db-*, prod-*"
              autocomplete="off"
              spellcheck="false"
              bind:value={twoPersonHostInput}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  draft.approval.two_person_hosts = addItem(draft.approval.two_person_hosts, twoPersonHostInput);
                  twoPersonHostInput = "";
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => {
                draft.approval.two_person_hosts = addItem(draft.approval.two_person_hosts, twoPersonHostInput);
                twoPersonHostInput = "";
              }}
            >
              <Plus size={14} />
              Add
            </button>
          </div>
          <span class="field-hint">
            Connection-name patterns that always require two approvals, regardless of tags.
          </span>
          {#if draft.approval.two_person_hosts.length > 0}
            <div class="flex flex-wrap gap-1.5">
              {#each draft.approval.two_person_hosts as h (h)}
                <span class="tag gap-1 pr-0.5">
                  {h}
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                    onclick={() => (draft.approval.two_person_hosts = removeItem(draft.approval.two_person_hosts, h))}
                    aria-label="Remove two-person host pattern {h}"
                  >
                    <X size={11} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
        </div>
      {:else if activeTab === "guardrails"}
        <div class="field">
          <label for="ai-read-paths" class="field-label">Readable paths</label>
          <div class="flex gap-2">
            <input
              id="ai-read-paths"
              type="text"
              class="input input-mono flex-1"
              placeholder="/var/log/**, /etc/nginx/**"
              autocomplete="off"
              spellcheck="false"
              bind:value={readPathInput}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  draft.read_paths = addItem(draft.read_paths, readPathInput);
                  readPathInput = "";
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => {
                draft.read_paths = addItem(draft.read_paths, readPathInput);
                readPathInput = "";
              }}
            >
              <Plus size={14} />
              Add
            </button>
          </div>
          <span class="field-hint">
            Leave empty to inherit built-in defaults ({builtInReadPaths.join(", ") || "/var/log/**, /etc/**, /srv/**"}).
          </span>
          {#if draft.read_paths.length > 0}
            <div class="flex flex-wrap gap-1.5">
              {#each draft.read_paths as p (p)}
                <span class="tag gap-1 pr-0.5">
                  {p}
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                    onclick={() => (draft.read_paths = removeItem(draft.read_paths, p))}
                    aria-label="Remove readable path {p}"
                  >
                    <X size={11} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
        </div>

        <div class="field">
          <label for="ai-write-paths" class="field-label">Writable paths</label>
          <div class="flex gap-2">
            <input
              id="ai-write-paths"
              type="text"
              class="input input-mono flex-1"
              placeholder="/etc/nginx/**, /srv/app/**"
              autocomplete="off"
              spellcheck="false"
              bind:value={writePathInput}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  draft.write_paths = addItem(draft.write_paths, writePathInput);
                  writePathInput = "";
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => {
                draft.write_paths = addItem(draft.write_paths, writePathInput);
                writePathInput = "";
              }}
            >
              <Plus size={14} />
              Add
            </button>
          </div>
          <span class="field-hint">
            When set, <code class="mono">edit_file</code> is denied outside these globs (and still requires approval inside them).
          </span>
          {#if draft.write_paths.length > 0}
            <div class="flex flex-wrap gap-1.5">
              {#each draft.write_paths as p (p)}
                <span class="tag gap-1 pr-0.5">
                  {p}
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                    onclick={() => (draft.write_paths = removeItem(draft.write_paths, p))}
                    aria-label="Remove writable path {p}"
                  >
                    <X size={11} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
        </div>

        <div class="field">
          <label for="ai-deny-paths" class="field-label">Extra blocked paths</label>
          <div class="flex gap-2">
            <input
              id="ai-deny-paths"
              type="text"
              class="input input-mono flex-1"
              placeholder="/var/lib/secrets/**, **/*.p12"
              autocomplete="off"
              spellcheck="false"
              bind:value={denyPathInput}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  draft.deny_paths = addItem(draft.deny_paths, denyPathInput);
                  denyPathInput = "";
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => {
                draft.deny_paths = addItem(draft.deny_paths, denyPathInput);
                denyPathInput = "";
              }}
            >
              <Plus size={14} />
              Add
            </button>
          </div>
          <span class="field-hint">Added on top of the built-in protected paths (never drops built-ins).</span>
          {#if draft.deny_paths.length > 0}
            <div class="flex flex-wrap gap-1.5">
              {#each draft.deny_paths as p (p)}
                <span class="tag gap-1 pr-0.5">
                  {p}
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                    onclick={() => (draft.deny_paths = removeItem(draft.deny_paths, p))}
                    aria-label="Remove blocked path {p}"
                  >
                    <X size={11} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
        </div>

        <div class="field">
          <label for="ai-deny-cmds" class="field-label">Extra blocked commands</label>
          <div class="flex gap-2">
            <input
              id="ai-deny-cmds"
              type="text"
              class="input input-mono flex-1"
              placeholder="kubectl delete *, DROP TABLE *"
              autocomplete="off"
              spellcheck="false"
              bind:value={denyCmdInput}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  draft.deny = addItem(draft.deny, denyCmdInput);
                  denyCmdInput = "";
                }
              }}
            />
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => {
                draft.deny = addItem(draft.deny, denyCmdInput);
                denyCmdInput = "";
              }}
            >
              <Plus size={14} />
              Add
            </button>
          </div>
          <span class="field-hint">Matched on the parsed command AST, added on top of built-in blocked commands.</span>
          {#if draft.deny.length > 0}
            <div class="flex flex-wrap gap-1.5">
              {#each draft.deny as cmd (cmd)}
                <span class="tag gap-1 pr-0.5">
                  {cmd}
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm size-5 text-muted hover:text-error"
                    onclick={() => (draft.deny = removeItem(draft.deny, cmd))}
                    aria-label="Remove blocked command {cmd}"
                  >
                    <X size={11} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
        </div>

        {#if builtInDenyPaths.length > 0 || builtInDenyCmds.length > 0}
          <div class="rounded-lg border border-border bg-panel">
            <button
              type="button"
              class="flex h-9 w-full cursor-pointer items-center gap-2 px-3 text-left text-xs font-medium text-secondary hover:text-primary"
              aria-expanded={showBuiltins}
              onclick={() => (showBuiltins = !showBuiltins)}
            >
              {#if showBuiltins}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
              <Shield size={13} class="text-accent" />
              Always-enforced built-in guardrails ({builtInDenyPaths.length} paths · {builtInDenyCmds.length} commands)
            </button>
            {#if showBuiltins}
              <div class="flex flex-col gap-3 border-t border-border-subtle p-3">
                <div>
                  <div class="eyebrow mb-1.5">Built-in protected paths</div>
                  <div class="flex flex-wrap gap-1">
                    {#each builtInDenyPaths as p (p)}
                      <span class="tag">{p}</span>
                    {/each}
                  </div>
                </div>
                <div>
                  <div class="eyebrow mb-1.5">Built-in blocked commands</div>
                  <div class="flex flex-wrap gap-1">
                    {#each builtInDenyCmds as c (c)}
                      <span class="tag">{c}</span>
                    {/each}
                  </div>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      {:else if activeTab === "budgets"}
        <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div class="field">
            <label for="ai-budget-calls" class="field-label">Calls per minute</label>
            <input
              id="ai-budget-calls"
              type="number"
              min="0"
              max="10000"
              class="input input-mono tabular-nums"
              bind:value={draft.budgets.calls_per_min}
            />
            <span class="field-hint">Rate limit across tool calls. 0 disables the limit.</span>
          </div>

          <div class="field">
            <label for="ai-budget-changes" class="field-label">Changes per run</label>
            <input
              id="ai-budget-changes"
              type="number"
              min="0"
              max="1000"
              class="input input-mono tabular-nums"
              bind:value={draft.budgets.changes_per_run}
            />
            <span class="field-hint">Maximum write/edit mutations per session (0 for observe).</span>
          </div>

          <div class="field">
            <label for="ai-budget-ttl" class="field-label">Run time limit</label>
            <input
              id="ai-budget-ttl"
              type="text"
              class="input input-mono"
              class:input-invalid={Boolean(ttlError)}
              placeholder="2h"
              spellcheck="false"
              bind:value={draft.budgets.run_ttl}
            />
            <div class="flex flex-wrap gap-1">
              {#each TTL_PRESETS as preset (preset)}
                <button
                  type="button"
                  class="btn btn-sm {draft.budgets.run_ttl.trim() === preset ? 'btn-secondary' : 'btn-ghost'}"
                  onclick={() => (draft.budgets.run_ttl = preset)}
                >
                  {preset}
                </button>
              {/each}
            </div>
            {#if ttlError}
              <span class="field-error">{ttlError}</span>
            {/if}
          </div>

          <div class="field">
            <label for="ai-budget-denials" class="field-label">Pause after denials / min</label>
            <input
              id="ai-budget-denials"
              type="number"
              min="0"
              max="100"
              class="input input-mono tabular-nums"
              bind:value={draft.budgets.max_denials_per_min}
            />
            <span class="field-hint">Circuit breaker threshold within one minute (0 turns it off).</span>
          </div>
        </div>

        <div class="flex flex-col gap-3 rounded-lg border border-border bg-panel p-4">
          <label for="ai-redact-vault" class="flex cursor-pointer items-center justify-between gap-4">
            <span class="flex min-w-0 flex-col gap-0.5">
              <span class="text-sm font-medium text-primary">Hide vault secrets from the agent</span>
              <span class="text-xs text-muted">
                Replace known secret values before command output is returned to the AI model.
              </span>
            </span>
            <input id="ai-redact-vault" type="checkbox" class="switch" bind:checked={draft.redact.vault_secrets} />
          </label>

          <div class="divider"></div>

          <div class="flex flex-wrap items-center justify-between gap-4">
            <div class="flex min-w-0 flex-col gap-0.5">
              <label for="ai-redact-patterns" class="text-sm font-medium text-primary">Secret pattern redaction</label>
              <span class="text-xs text-muted">Automatically mask AWS keys, private keys, and bearer tokens.</span>
            </div>
            <div class="w-48">
              <CustomSelect
                id="ai-redact-patterns"
                ariaLabel="Secret pattern redaction"
                options={REDACT_OPTIONS}
                bind:value={draft.redact.patterns}
              />
            </div>
          </div>
        </div>
      {/if}
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={onClose} disabled={saving}>Cancel</button>
      <button type="submit" class="btn btn-primary" disabled={saving}>
        {#if saving}<span class="spinner size-3" aria-hidden="true"></span>{/if}
        {isEditing ? "Save policy" : "Create policy"}
      </button>
    </div>
  </form>
</ModalShell>
