<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    Check,
    ChevronRight,
    Copy,
    Download,
    Maximize2,
    Minimize2,
    Play,
    Plus,
    Search,
    Trash2,
    TriangleAlert,
    X,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";

  interface BatchExecHostResult {
    connection_id: string;
    name: string;
    host: string;
    user: string;
    is_production: boolean;
    stdout: string;
    stderr: string;
    exit_code: number;
    success: boolean;
    duration_ms: number;
  }

  interface EnvStatus {
    ssh_agent_available: boolean;
    ssh_auth_sock: string | null;
    kerberos_available: boolean;
    warnings: string[];
  }

  interface BatchRunRecord {
    id: string;
    timestamp: number;
    command: string;
    targetCount: number;
    dryRun: boolean;
    results: BatchExecHostResult[];
    durationMs: number;
  }

  interface CommandTemplate {
    id: string;
    name: string;
    command: string;
  }

  interface Props {
    show: boolean;
    connections: Connection[];
    onClose: () => void;
  }

  let { show, connections, onClose }: Props = $props();

  const DEFAULT_TEMPLATES: CommandTemplate[] = [
    { id: "uptime", name: "Uptime", command: "uptime" },
    { id: "disk", name: "Disk usage", command: "df -h" },
    { id: "memory", name: "Memory", command: "free -m" },
    { id: "load", name: "Load average", command: "cat /proc/loadavg" },
    { id: "users", name: "Logged-in users", command: "w" },
    { id: "docker", name: "Containers", command: "docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Image}}'" },
    { id: "services", name: "Failed services", command: "systemctl --failed" },
    { id: "ssh-ver", name: "SSH version", command: "ssh -V 2>&1" },
  ];
  const BUILTIN_TEMPLATE_IDS = new Set(DEFAULT_TEMPLATES.map((t) => t.id));

  const EXPORT_OPTIONS = [
    { value: "md", label: "Markdown" },
    { value: "csv", label: "CSV" },
    { value: "json", label: "JSON" },
    { value: "txt", label: "Plain text" },
  ];

  // ─── Modal state ─────────────────────────────────────────────────────────────
  let activeTab = $state<"execute" | "history" | "templates">("execute");
  let isFullscreen = $state(false);
  let initialized = $state(false);

  // ─── Execution state ─────────────────────────────────────────────────────────
  const selectedIds = new SvelteSet<string>();
  let hostFilter = $state("");
  let tagFilter = $state<string | null>(null);
  let command = $state("uptime");
  let dryRun = $state(true);
  let timeoutSecs = $state(10);
  let running = $state(false);
  let results = $state.raw<BatchExecHostResult[] | null>(null);
  const expandedResults = new SvelteSet<string>();
  let outputTab = $state<"all" | "stdout" | "stderr">("all");
  let copiedId = $state<string | null>(null);
  let exportFormat = $state<"md" | "csv" | "json" | "txt">("md");

  // ─── Templates & history ─────────────────────────────────────────────────────
  let history = $state.raw<BatchRunRecord[]>([]);
  let templates = $state.raw<CommandTemplate[]>([]);
  let newTemplateName = $state("");
  let newTemplateCmd = $state("");

  // ─── Environment status ──────────────────────────────────────────────────────
  let envStatus = $state<EnvStatus | null>(null);
  let envWarningDismissed = $state(false);

  $effect(() => {
    if (show && !initialized) {
      untrack(() => replaceSelection(connections.map((c) => c.id)));
      initialized = true;
      invoke<EnvStatus>("get_env_status")
        .then((s) => (envStatus = s))
        .catch(() => {});
      loadHistory();
      loadTemplates();
    } else if (!show) {
      initialized = false;
      results = null;
      envWarningDismissed = false;
    }
  });

  const prodIds = $derived(
    new Set(
      connections
        .filter((c) => c.name.toLowerCase().includes("prod") || c.tags.some((t) => t.toLowerCase().includes("prod")))
        .map((c) => c.id),
    ),
  );

  const allTags = $derived.by(() => {
    const set = new Set<string>();
    for (const c of connections) for (const t of c.tags) set.add(t);
    return Array.from(set).sort();
  });

  const filteredConnections = $derived.by(() => {
    const q = hostFilter.trim().toLowerCase();
    if (!q && !tagFilter) return connections;
    return connections.filter(
      (c) =>
        (!tagFilter || c.tags.includes(tagFilter)) &&
        (!q ||
          c.name.toLowerCase().includes(q) ||
          c.host.toLowerCase().includes(q) ||
          c.user.toLowerCase().includes(q) ||
          c.tags.some((t) => t.toLowerCase().includes(q))),
    );
  });

  const targetedConnections = $derived(connections.filter((c) => selectedIds.has(c.id)));
  const hasProductionTarget = $derived(targetedConnections.some((c) => prodIds.has(c.id)));
  const isFiltered = $derived(hostFilter.trim() !== "" || tagFilter !== null);
  const failedCount = $derived(results ? results.filter((r) => !r.success).length : 0);

  const showEnvWarning = $derived(
    !envWarningDismissed && (envStatus?.warnings?.length ?? 0) > 0,
  );

  function toggleSelect(id: string) {
    if (selectedIds.has(id)) selectedIds.delete(id);
    else selectedIds.add(id);
  }

  function replaceSelection(ids: Iterable<string>) {
    selectedIds.clear();
    for (const id of ids) selectedIds.add(id);
  }

  /** Selects every visible host (all hosts when no filter is active). */
  function selectAll() {
    for (const c of isFiltered ? filteredConnections : connections) selectedIds.add(c.id);
  }

  /** Clears the visible hosts (all hosts when no filter is active). */
  function selectNone() {
    if (!isFiltered) {
      selectedIds.clear();
      return;
    }
    for (const c of filteredConnections) selectedIds.delete(c.id);
  }

  function selectInvert() {
    replaceSelection(connections.filter((c) => !selectedIds.has(c.id)).map((c) => c.id));
  }

  function selectProdOnly() {
    replaceSelection(prodIds);
  }

  function selectNonProd() {
    replaceSelection(connections.filter((c) => !prodIds.has(c.id)).map((c) => c.id));
  }

  // ─── Storage helpers ─────────────────────────────────────────────────────────
  function loadHistory() {
    try {
      const raw = localStorage.getItem("bayesian-ssh-batch-history");
      if (raw) history = JSON.parse(raw).slice(0, 100);
    } catch {
      history = [];
    }
  }

  function saveHistory() {
    try {
      localStorage.setItem("bayesian-ssh-batch-history", JSON.stringify(history.slice(0, 100)));
    } catch {
      // ignore storage quota
    }
  }

  function clearHistory() {
    history = [];
    try {
      localStorage.removeItem("bayesian-ssh-batch-history");
    } catch {}
    notify("Batch history cleared", "info");
  }

  function loadTemplates() {
    try {
      const raw = localStorage.getItem("bayesian-ssh-batch-templates");
      const parsed = raw ? (JSON.parse(raw) as CommandTemplate[]) : [];
      templates = [...DEFAULT_TEMPLATES, ...parsed.filter((t) => !BUILTIN_TEMPLATE_IDS.has(t.id))];
    } catch {
      templates = [...DEFAULT_TEMPLATES];
    }
  }

  function saveTemplates() {
    try {
      const custom = templates.filter((t) => !BUILTIN_TEMPLATE_IDS.has(t.id));
      localStorage.setItem("bayesian-ssh-batch-templates", JSON.stringify(custom));
    } catch {}
  }

  function addTemplate() {
    if (!newTemplateName.trim() || !newTemplateCmd.trim()) return;
    templates = [
      ...templates,
      { id: `user-${Date.now()}`, name: newTemplateName.trim(), command: newTemplateCmd.trim() },
    ];
    saveTemplates();
    newTemplateName = "";
    newTemplateCmd = "";
    notify("Template added", "success");
  }

  function deleteTemplate(id: string) {
    templates = templates.filter((t) => t.id !== id);
    saveTemplates();
    notify("Template removed", "info");
  }

  function applyTemplate(cmd: string) {
    command = cmd;
    activeTab = "execute";
  }

  function showResults(res: BatchExecHostResult[]) {
    results = res;
    outputTab = "all";
    expandedResults.clear();
    // Few hosts: open everything. Otherwise open only the failures.
    for (const r of res) if (res.length <= 3 || !r.success) expandedResults.add(r.connection_id);
  }

  // ─── Execution ───────────────────────────────────────────────────────────────
  async function executeBatch() {
    if (selectedIds.size === 0) {
      notify("Select at least one host.", "error");
      return;
    }
    if (!command.trim()) {
      notify("Enter a command to run.", "error");
      return;
    }

    const connectionIds = targetedConnections.map((c) => c.id);
    running = true;
    results = null;
    const startedAt = Date.now();
    try {
      const res = await invoke<BatchExecHostResult[]>("run_batch_command", {
        connectionIds,
        command: command.trim(),
        dryRun,
        timeoutSecs: Number(timeoutSecs),
      });
      showResults(res);

      const record: BatchRunRecord = {
        id: crypto.randomUUID(),
        timestamp: startedAt,
        command: command.trim(),
        targetCount: connectionIds.length,
        dryRun,
        results: res,
        durationMs: Date.now() - startedAt,
      };
      history = [record, ...history].slice(0, 100);
      saveHistory();

      notify(
        dryRun ? `Dry run previewed on ${res.length} host(s)` : `Command ran on ${res.length} host(s)`,
        "success",
      );
    } catch (err) {
      notify(`Batch execution failed: ${err}`, "error");
    } finally {
      running = false;
    }
  }

  function exportResults(): void {
    if (!results || results.length === 0) return;
    let content: string;
    if (exportFormat === "json") {
      content = JSON.stringify({ command, dryRun, results }, null, 2);
    } else if (exportFormat === "csv") {
      const esc = (s: string) => `"${(s ?? "").replace(/"/g, '""')}"`;
      const lines = ["name,host,user,exit_code,duration_ms,success,stdout,stderr"];
      for (const r of results) {
        lines.push(
          [esc(r.name), esc(r.host), esc(r.user), r.exit_code, r.duration_ms, r.success, esc(r.stdout), esc(r.stderr)].join(","),
        );
      }
      content = lines.join("\n");
    } else if (exportFormat === "txt") {
      content = results
        .map(
          (r) =>
            `=== ${r.name} (${r.user}@${r.host}) ===\nexit: ${r.exit_code} | duration: ${r.duration_ms}ms | success: ${r.success}\n\nSTDOUT:\n${r.stdout || "(none)"}\n\nSTDERR:\n${r.stderr || "(none)"}\n`,
        )
        .join("\n------------------------------------------------------------\n\n");
    } else {
      content = results
        .map(
          (r) =>
            `| ${r.name} | \`${r.user}@${r.host}\` | ${r.exit_code} | ${r.duration_ms}ms | ${r.success ? "Passed" : "Failed"} |`,
        )
        .join("\n");
      content = `# Batch Execution Report: \`${command}\`\n\n- Mode: ${dryRun ? "Dry Run (Preview)" : "Live Execution"}\n- Total Hosts: ${results.length}\n- Timestamp: ${new Date().toISOString()}\n\n| Host | Target | Exit Code | Latency | Status |\n|---|---|---|---|---|\n${content}`;
    }

    const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `batch_${new Date().toISOString().slice(0, 19).replace(/[:.]/g, "-")}.${exportFormat}`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function copyOutput(r: BatchExecHostResult) {
    copyTextWithFallback(
      `Host: ${r.name} (${r.user}@${r.host})\nExit Code: ${r.exit_code}\n\nSTDOUT:\n${r.stdout}\n\nSTDERR:\n${r.stderr}`,
    );
    copiedId = r.connection_id;
    setTimeout(() => (copiedId = null), 2000);
  }

  function toggleExpanded(id: string) {
    if (expandedResults.has(id)) expandedResults.delete(id);
    else expandedResults.add(id);
  }

  function handleModalKeydown(e: KeyboardEvent) {
    const isMod = e.ctrlKey || e.metaKey;
    if (isMod && e.key === "Enter" && !running) {
      e.preventDefault();
      void executeBatch();
      return;
    }
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (isMod && e.shiftKey && e.key.toLowerCase() === "d") {
      e.preventDefault();
      dryRun = !dryRun;
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (show) handleModalKeydown(e); }} />

{#snippet resultOutput(r: BatchExecHostResult)}
  {@const showOut = outputTab !== "stderr" && r.stdout}
  {@const showErr = outputTab !== "stdout" && r.stderr}
  <div class="relative">
    <button
      type="button"
      class="btn-icon btn-icon-sm absolute right-1.5 top-1.5"
      aria-label="Copy output of {r.name}"
      title="Copy output"
      onclick={() => copyOutput(r)}
    >
      {#if copiedId === r.connection_id}
        <Check size={14} class="text-success" />
      {:else}
        <Copy size={14} />
      {/if}
    </button>
    <div class="code-block max-h-72 overflow-y-auto pr-9">
      {#if showOut}
        <pre class="m-0 whitespace-pre-wrap break-words font-mono">{r.stdout}</pre>
      {/if}
      {#if showErr}
        <pre class="m-0 whitespace-pre-wrap break-words font-mono text-error {showOut ? 'mt-2 border-t border-border pt-2' : ''}">{r.stderr}</pre>
      {/if}
      {#if !showOut && !showErr}
        <span class="text-muted">No {outputTab === "all" ? "output" : outputTab} (exit {r.exit_code})</span>
      {/if}
    </div>
  </div>
{/snippet}

{#if show}
  <ModalShell
    open={show}
    title="Run command on hosts"
    {onClose}
    width={isFullscreen ? "full" : "lg"}
    panelStyle={isFullscreen ? "" : "max-width: 1120px; height: min(780px, calc(100dvh - 48px));"}
    overlayStyle={isFullscreen ? "padding: 0" : ""}
  >
    <header class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Run command on hosts</h2>
        <p class="modal-subtitle">Run one command on several hosts in parallel. Preview with a dry run first.</p>
      </div>
      <div class="flex shrink-0 items-center gap-0.5">
        <button
          type="button"
          class="btn-icon"
          aria-label={isFullscreen ? "Restore size" : "Maximize"}
          title={isFullscreen ? "Restore size" : "Maximize"}
          onclick={() => (isFullscreen = !isFullscreen)}
        >
          {#if isFullscreen}<Minimize2 size={14} />{:else}<Maximize2 size={14} />{/if}
        </button>
        <button type="button" class="modal-close mr-0 mt-0" aria-label="Close" onclick={onClose}>
          <X size={16} />
        </button>
      </div>
    </header>

    <nav class="tabs shrink-0 px-5" aria-label="Batch sections">
      <button type="button" class="tab {activeTab === 'execute' ? 'tab-active' : ''}" onclick={() => (activeTab = "execute")}>
        Run
      </button>
      <button type="button" class="tab {activeTab === 'history' ? 'tab-active' : ''}" onclick={() => (activeTab = "history")}>
        History
        {#if history.length > 0}<span class="count">{history.length}</span>{/if}
      </button>
      <button type="button" class="tab {activeTab === 'templates' ? 'tab-active' : ''}" onclick={() => (activeTab = "templates")}>
        Templates
      </button>
    </nav>

    {#if showEnvWarning}
      <div class="alert alert-warning mx-5 mt-3 shrink-0 items-center">
        <TriangleAlert size={14} class="shrink-0" />
        <span class="min-w-0 flex-1"><span class="alert-title">SSH environment:</span> {envStatus?.warnings[0]}</span>
        <button
          type="button"
          class="btn-icon btn-icon-sm text-warning"
          aria-label="Dismiss warning"
          onclick={() => (envWarningDismissed = true)}
        >
          <X size={14} />
        </button>
      </div>
    {/if}

    {#if activeTab === "execute"}
      <div class="flex min-h-0 flex-1 overflow-hidden">
        <!-- Host picker -->
        <aside class="flex w-72 shrink-0 flex-col border-r border-border">
          <div class="flex flex-col gap-2 px-4 pb-2 pt-4">
            <div class="flex items-center justify-between">
              <span class="section-label">Hosts</span>
              <span class="text-xs tabular-nums text-muted">{selectedIds.size} of {connections.length} selected</span>
            </div>
            <label class="search-box">
              <Search size={14} class="shrink-0" />
              <input type="text" bind:value={hostFilter} placeholder="Filter hosts" aria-label="Filter hosts" />
            </label>
            {#if allTags.length > 0}
              <div class="flex flex-wrap gap-1">
                {#each allTags as tag (tag)}
                  <button
                    type="button"
                    class="chip h-6 px-2 {tagFilter === tag ? 'chip-active' : ''}"
                    aria-pressed={tagFilter === tag}
                    onclick={() => (tagFilter = tagFilter === tag ? null : tag)}
                  >
                    {tag}
                  </button>
                {/each}
              </div>
            {/if}
            <div class="-mx-1 flex items-center gap-0.5">
              <button type="button" class="btn btn-ghost btn-sm px-1.5" onclick={selectAll}>All</button>
              <button type="button" class="btn btn-ghost btn-sm px-1.5" onclick={selectNone}>None</button>
              <button type="button" class="btn btn-ghost btn-sm px-1.5" onclick={selectInvert}>Invert</button>
              <span class="mx-1 h-4 w-px bg-border"></span>
              <button type="button" class="btn btn-ghost btn-sm px-1.5" onclick={selectProdOnly}>Prod</button>
              <button type="button" class="btn btn-ghost btn-sm px-1.5" onclick={selectNonProd}>Non-prod</button>
            </div>
          </div>

          <div class="min-h-0 flex-1 overflow-y-auto border-t border-border-subtle px-2 py-1.5">
            {#each filteredConnections as conn (conn.id)}
              <label
                class="flex h-11 cursor-pointer items-center gap-2.5 rounded-md px-2 transition-colors duration-fast hover:bg-surface-hover"
              >
                <input
                  type="checkbox"
                  class="checkbox"
                  checked={selectedIds.has(conn.id)}
                  onchange={() => toggleSelect(conn.id)}
                />
                <span class="flex min-w-0 flex-1 flex-col">
                  <span class="truncate text-sm text-primary">{conn.name}</span>
                  <span class="truncate font-mono text-xs text-muted">{conn.user}@{conn.host}</span>
                </span>
                {#if prodIds.has(conn.id)}
                  <span class="badge badge-warning">prod</span>
                {/if}
              </label>
            {:else}
              <p class="m-0 px-2 py-8 text-center text-xs text-muted">No hosts match the filter.</p>
            {/each}
          </div>
        </aside>

        <!-- Command + results -->
        <section class="flex min-w-0 flex-1 flex-col">
          <div class="flex shrink-0 flex-col gap-3 border-b border-border p-4">
            {#if hasProductionTarget && !dryRun}
              <div class="alert alert-warning">
                <TriangleAlert size={14} class="mt-0.5 shrink-0" />
                <span>
                  <span class="alert-title">Production hosts selected.</span>
                  Dry run is off, so the command runs for real on production.
                </span>
              </div>
            {/if}

            <div class="field">
              <div class="flex items-center justify-between">
                <label class="field-label" for="exec-command-input">Command</label>
                <span class="flex items-center gap-1 text-xs text-muted">
                  <kbd class="kbd">Ctrl</kbd><kbd class="kbd">↵</kbd> to run
                </span>
              </div>
              <textarea
                id="exec-command-input"
                class="input input-mono resize-y"
                rows="3"
                spellcheck="false"
                bind:value={command}
                placeholder="e.g. uptime, systemctl status nginx, df -h"
              ></textarea>
            </div>

            <div class="flex flex-wrap items-center gap-1.5">
              {#each templates.slice(0, 8) as tmpl (tmpl.id)}
                <button
                  type="button"
                  class="chip h-6 px-2 {command === tmpl.command ? 'chip-active' : ''}"
                  title={tmpl.command}
                  onclick={() => (command = tmpl.command)}
                >
                  {tmpl.name}
                </button>
              {/each}
              <button
                type="button"
                class="link ml-auto flex items-center gap-1 text-xs"
                disabled={!command.trim()}
                onclick={() => {
                  newTemplateCmd = command.trim();
                  newTemplateName = command.trim().split(" ")[0] || "Custom";
                  activeTab = "templates";
                }}
              >
                <Plus size={14} />
                Save as template
              </button>
            </div>

            <div class="flex items-center gap-4">
              <label class="flex cursor-pointer items-center gap-2 text-sm text-secondary">
                <input type="checkbox" class="switch" bind:checked={dryRun} />
                Dry run
              </label>
              <label class="flex items-center gap-2 text-sm text-secondary">
                Timeout
                <input
                  type="number"
                  min="1"
                  max="300"
                  class="input input-mono h-7 w-16 text-center"
                  bind:value={timeoutSecs}
                  aria-label="Timeout in seconds"
                />
                <span class="text-xs text-muted">s</span>
              </label>
              <button
                type="button"
                class="btn ml-auto {dryRun || !hasProductionTarget ? 'btn-primary' : 'btn-danger'}"
                onclick={executeBatch}
                disabled={running || selectedIds.size === 0}
              >
                {#if running}
                  <span class="spinner"></span>
                  Running on {selectedIds.size}…
                {:else}
                  <Play size={14} />
                  {dryRun ? "Preview" : "Run"} on {selectedIds.size} host{selectedIds.size === 1 ? "" : "s"}
                {/if}
              </button>
            </div>
          </div>

          <div class="min-h-0 flex-1 overflow-y-auto p-4">
            {#if running}
              <div class="empty-state py-12">
                <span class="spinner mb-3 size-5 text-accent"></span>
                <div class="empty-state-title">Running on {selectedIds.size} host{selectedIds.size === 1 ? "" : "s"}</div>
                <div class="empty-state-desc">Each host times out after {timeoutSecs}s.</div>
              </div>
            {:else if results}
              <div class="mb-2 flex items-center gap-3">
                <span class="text-sm font-medium text-primary">Results</span>
                <span class="flex items-center gap-1.5 text-xs tabular-nums text-muted">
                  <span>{results.length - failedCount} passed</span>
                  {#if failedCount > 0}
                    <span aria-hidden="true">·</span>
                    <span class="text-error">{failedCount} failed</span>
                  {/if}
                </span>
                <div class="ml-auto flex items-center gap-2">
                  <div class="segmented h-7" role="group" aria-label="Output stream">
                    {#each [["all", "All"], ["stdout", "stdout"], ["stderr", "stderr"]] as [value, label] (value)}
                      <button
                        type="button"
                        class="segmented-item {outputTab === value ? 'segmented-item-active' : ''}"
                        aria-pressed={outputTab === value}
                        onclick={() => (outputTab = value as typeof outputTab)}
                      >
                        {label}
                      </button>
                    {/each}
                  </div>
                  <CustomSelect
                    options={EXPORT_OPTIONS}
                    value={exportFormat}
                    onChange={(v) => (exportFormat = v as typeof exportFormat)}
                    size="sm"
                    class="w-32"
                  />
                  <button type="button" class="btn btn-secondary btn-sm" onclick={exportResults}>
                    <Download size={14} />
                    Export
                  </button>
                </div>
              </div>

              <div class="table-wrap">
                {#each results as r, i (r.connection_id)}
                  {@const open = expandedResults.has(r.connection_id)}
                  <div class={i > 0 ? "border-t border-border-subtle" : ""}>
                    <button
                      type="button"
                      class="flex h-11 w-full cursor-pointer items-center gap-3 px-4 text-left text-sm transition-colors duration-fast hover:bg-surface-hover"
                      aria-expanded={open}
                      onclick={() => toggleExpanded(r.connection_id)}
                    >
                      <ChevronRight
                        size={14}
                        class="shrink-0 text-muted transition-transform duration-fast {open ? 'rotate-90' : ''}"
                      />
                      <span class="w-16 shrink-0">
                        <span class="badge {r.success ? 'badge-success' : 'badge-error'}">exit {r.exit_code}</span>
                      </span>
                      <span class="truncate text-primary">{r.name}</span>
                      <span class="min-w-0 truncate font-mono text-xs text-muted">{r.user}@{r.host}</span>
                      <span class="ml-auto shrink-0 font-mono text-xs tabular-nums text-muted">{r.duration_ms} ms</span>
                    </button>
                    {#if open}
                      <div class="px-4 pb-3 pl-11">
                        {@render resultOutput(r)}
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else}
              <div class="empty-state py-12">
                <div class="empty-state-title">No results yet</div>
                <div class="empty-state-desc">Pick hosts, enter a command, then preview it with a dry run before running it for real.</div>
              </div>
            {/if}
          </div>
        </section>
      </div>
    {:else if activeTab === "history"}
      <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-5">
        <div class="flex items-center justify-between">
          <span class="text-xs text-muted">Last {history.length} batch run{history.length === 1 ? "" : "s"} in this workspace</span>
          {#if history.length > 0}
            <button type="button" class="btn btn-danger-ghost btn-sm" onclick={clearHistory}>
              <Trash2 size={14} />
              Clear history
            </button>
          {/if}
        </div>

        {#if history.length > 0}
          <div class="table-wrap min-h-0 overflow-y-auto">
            <table class="data-table">
              <thead>
                <tr>
                  <th>Command</th>
                  <th class="w-24">Mode</th>
                  <th class="w-28">Hosts</th>
                  <th class="w-20">Duration</th>
                  <th class="w-44">When</th>
                  <th class="w-40"><span class="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                {#each history as item (item.id)}
                  {@const passed = item.results?.filter((r) => r.success).length ?? 0}
                  <tr>
                    <td class="max-w-0"><span class="block truncate font-mono text-xs text-primary">{item.command}</span></td>
                    <td><span class="badge {item.dryRun ? 'badge-neutral' : 'badge-accent'}">{item.dryRun ? "Dry run" : "Live"}</span></td>
                    <td class="tabular-nums">
                      <span class={passed < item.targetCount ? "text-error" : ""}>{passed}/{item.targetCount}</span> passed
                    </td>
                    <td class="font-mono text-xs tabular-nums">{item.durationMs} ms</td>
                    <td class="text-xs">{new Date(item.timestamp).toLocaleString()}</td>
                    <td>
                      <div class="row-actions">
                        <button
                          type="button"
                          class="btn btn-ghost btn-sm"
                          onclick={() => {
                            command = item.command;
                            dryRun = item.dryRun;
                            showResults(item.results);
                            activeTab = "execute";
                          }}
                        >
                          Inspect
                        </button>
                        <button
                          type="button"
                          class="btn btn-secondary btn-sm"
                          onclick={() => {
                            command = item.command;
                            dryRun = item.dryRun;
                            activeTab = "execute";
                            void executeBatch();
                          }}
                        >
                          Run again
                        </button>
                      </div>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <div class="empty-state">
            <div class="empty-state-title">No batch runs yet</div>
            <div class="empty-state-desc">Runs and dry runs appear here so you can inspect or repeat them.</div>
          </div>
        {/if}
      </div>
    {:else}
      <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-5">
        <form
          class="flex shrink-0 items-end gap-2"
          onsubmit={(e) => {
            e.preventDefault();
            addTemplate();
          }}
        >
          <div class="field w-52">
            <label class="field-label" for="tmpl-name">Name</label>
            <input id="tmpl-name" class="input" bind:value={newTemplateName} placeholder="Health check" />
          </div>
          <div class="field flex-1">
            <label class="field-label" for="tmpl-cmd">Command</label>
            <input
              id="tmpl-cmd"
              class="input input-mono"
              bind:value={newTemplateCmd}
              placeholder="curl -sI https://localhost"
              spellcheck="false"
            />
          </div>
          <button type="submit" class="btn btn-secondary" disabled={!newTemplateName.trim() || !newTemplateCmd.trim()}>
            <Plus size={14} />
            Add template
          </button>
        </form>

        <div class="table-wrap min-h-0 overflow-y-auto">
          <table class="data-table">
            <thead>
              <tr>
                <th class="w-48">Name</th>
                <th>Command</th>
                <th class="w-24">Type</th>
                <th class="w-36"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each templates as tmpl (tmpl.id)}
                {@const builtin = BUILTIN_TEMPLATE_IDS.has(tmpl.id)}
                <tr>
                  <td class="text-primary">{tmpl.name}</td>
                  <td class="max-w-0"><span class="block truncate font-mono text-xs" title={tmpl.command}>{tmpl.command}</span></td>
                  <td><span class="badge {builtin ? 'badge-neutral' : 'badge-accent'}">{builtin ? "Built-in" : "Custom"}</span></td>
                  <td>
                    <div class="row-actions">
                      {#if !builtin}
                        <button
                          type="button"
                          class="btn-icon btn-icon-danger"
                          aria-label="Delete template {tmpl.name}"
                          onclick={() => deleteTemplate(tmpl.id)}
                        >
                          <Trash2 size={14} />
                        </button>
                      {/if}
                      <button type="button" class="btn btn-secondary btn-sm" onclick={() => applyTemplate(tmpl.command)}>
                        Use
                      </button>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>
    {/if}
  </ModalShell>
{/if}
