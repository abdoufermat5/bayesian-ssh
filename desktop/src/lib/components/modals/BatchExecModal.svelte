<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    AlertCircle,
    Bookmark,
    Check,
    CheckCircle2,
    Clock,
    Copy,
    Download,
    Maximize2,
    Minimize2,
    Play,
    Plus,
    RefreshCw,
    Search,
    Server,
    ShieldAlert,
    Terminal,
    Trash2,
    TriangleAlert,
    X,
    XCircle,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

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
    { id: "uptime", name: "System Uptime", command: "uptime" },
    { id: "disk", name: "Disk Usage", command: "df -h" },
    { id: "memory", name: "Memory Stats", command: "free -m" },
    { id: "load", name: "Load Average", command: "cat /proc/loadavg" },
    { id: "users", name: "Logged-in Users", command: "w" },
    { id: "docker", name: "Containers", command: "docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Image}}'" },
    { id: "services", name: "Failed Services", command: "systemctl --failed" },
    { id: "ssh-ver", name: "SSH Version", command: "ssh -V 2>&1" },
  ];

  // ─── Modal State ─────────────────────────────────────────────────────────────
  let activeTab = $state<"execute" | "history" | "templates">("execute");
  let isFullscreen = $state<boolean>(false);
  let initialized = $state<boolean>(false);

  // ─── Execution State ─────────────────────────────────────────────────────────
  let selectedIds = $state<string[]>([]);
  let hostFilter = $state<string>("");
  let command = $state<string>("uptime");
  let dryRun = $state<boolean>(true);
  let timeoutSecs = $state<number>(10);
  let running = $state<boolean>(false);
  let results = $state<BatchExecHostResult[] | null>(null);
  let activeResultIndex = $state<number>(0);
  let outputTab = $state<"all" | "stdout" | "stderr">("all");
  let copied = $state<boolean>(false);
  let exportFormat = $state<"md" | "csv" | "json" | "txt">("md");

  // ─── Templates & History ─────────────────────────────────────────────────────
  let history = $state<BatchRunRecord[]>([]);
  let templates = $state<CommandTemplate[]>([]);
  let newTemplateName = $state<string>("");
  let newTemplateCmd = $state<string>("");

  // ─── Environment Status ──────────────────────────────────────────────────────
  let envStatus = $state<EnvStatus | null>(null);
  let envWarningDismissed = $state<boolean>(false);

  $effect(() => {
    if (show && !initialized) {
      selectedIds = connections.map((c) => c.id);
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

  const filteredConnections = $derived.by(() => {
    const q = hostFilter.trim().toLowerCase();
    if (!q) return connections;
    return connections.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        c.host.toLowerCase().includes(q) ||
        c.user.toLowerCase().includes(q) ||
        c.tags.some((t) => t.toLowerCase().includes(q)),
    );
  });

  const targetedConnections = $derived.by(() =>
    connections.filter((c) => selectedIds.includes(c.id)),
  );

  const hasProductionTarget = $derived.by(() =>
    targetedConnections.some(
      (c) =>
        c.name.toLowerCase().includes("prod") ||
        c.tags.some((t) => t.toLowerCase().includes("prod")),
    ),
  );

  const showEnvWarning = $derived.by(
    () =>
      !envWarningDismissed &&
      envStatus !== null &&
      envStatus.warnings &&
      envStatus.warnings.length > 0,
  );

  function toggleSelect(id: string) {
    if (selectedIds.includes(id)) {
      selectedIds = selectedIds.filter((i) => i !== id);
    } else {
      selectedIds = [...selectedIds, id];
    }
  }

  function selectAll() {
    selectedIds = connections.map((c) => c.id);
  }

  function selectNone() {
    selectedIds = [];
  }

  function selectInvert() {
    const set = new Set(selectedIds);
    selectedIds = connections.filter((c) => !set.has(c.id)).map((c) => c.id);
  }

  function selectProdOnly() {
    selectedIds = connections
      .filter(
        (c) =>
          c.name.toLowerCase().includes("prod") ||
          c.tags.some((t) => t.toLowerCase().includes("prod")),
      )
      .map((c) => c.id);
  }

  function selectNonProd() {
    selectedIds = connections
      .filter(
        (c) =>
          !c.name.toLowerCase().includes("prod") &&
          !c.tags.some((t) => t.toLowerCase().includes("prod")),
      )
      .map((c) => c.id);
  }

  // ─── Storage Helpers ─────────────────────────────────────────────────────────
  function loadHistory() {
    try {
      const raw = localStorage.getItem("bayesian-ssh-batch-history");
      if (raw) history = JSON.parse(raw).slice(-100);
    } catch {
      history = [];
    }
  }

  function saveHistory() {
    try {
      localStorage.setItem("bayesian-ssh-batch-history", JSON.stringify(history.slice(-100)));
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
      if (raw) {
        const parsed = JSON.parse(raw) as CommandTemplate[];
        templates = [
          ...DEFAULT_TEMPLATES,
          ...parsed.filter((t) => !DEFAULT_TEMPLATES.some((d) => d.id === t.id)),
        ];
      } else {
        templates = [...DEFAULT_TEMPLATES];
      }
    } catch {
      templates = [...DEFAULT_TEMPLATES];
    }
  }

  function saveTemplates() {
    try {
      const custom = templates.filter((t) => !DEFAULT_TEMPLATES.some((d) => d.id === t.id));
      localStorage.setItem("bayesian-ssh-batch-templates", JSON.stringify(custom));
    } catch {}
  }

  function addTemplate() {
    if (!newTemplateName.trim() || !newTemplateCmd.trim()) return;
    const item: CommandTemplate = {
      id: `user-${Date.now()}`,
      name: newTemplateName.trim(),
      command: newTemplateCmd.trim(),
    };
    templates = [...templates, item];
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
    notify("Template applied to runner", "info");
  }

  // ─── Execution ───────────────────────────────────────────────────────────────
  async function executeBatch() {
    if (selectedIds.length === 0) {
      notify("Please select at least one target host.", "error");
      return;
    }
    if (!command.trim()) {
      notify("Please enter a command to execute.", "error");
      return;
    }

    running = true;
    results = null;
    const startedAt = Date.now();
    try {
      const res = await invoke<BatchExecHostResult[]>("run_batch_command", {
        connectionIds: selectedIds,
        command: command.trim(),
        dryRun,
        timeoutSecs: Number(timeoutSecs),
      });
      results = res;
      activeResultIndex = 0;
      outputTab = "all";

      const record: BatchRunRecord = {
        id: crypto.randomUUID(),
        timestamp: startedAt,
        command: command.trim(),
        targetCount: selectedIds.length,
        dryRun,
        results: res,
        durationMs: Date.now() - startedAt,
      };
      history = [record, ...history].slice(0, 100);
      saveHistory();

      notify(
        dryRun
          ? `Dry-run preview generated for ${res.length} host(s).`
          : `Executed command on ${res.length} host(s).`,
        "success",
      );
    } catch (err) {
      notify(`Batch execution error: ${err}`, "error");
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
        lines.push([
          esc(r.name),
          esc(r.host),
          esc(r.user),
          r.exit_code,
          r.duration_ms,
          r.success,
          esc(r.stdout),
          esc(r.stderr),
        ].join(","));
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
    const ext = exportFormat === "csv" ? "csv" : exportFormat === "json" ? "json" : exportFormat === "txt" ? "txt" : "md";
    a.download = `batch_${new Date().toISOString().slice(0, 19).replace(/[:.]/g, "-")}.${ext}`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function copyActiveOutput() {
    if (!results || !results[activeResultIndex]) return;
    const active = results[activeResultIndex];
    const text = `Host: ${active.name} (${active.user}@${active.host})\nExit Code: ${active.exit_code}\n\nSTDOUT:\n${active.stdout}\n\nSTDERR:\n${active.stderr}`;
    navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  function handleModalKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    const isMod = e.ctrlKey || e.metaKey;
    if (isMod && e.key === "Enter" && !running) {
      e.preventDefault();
      void executeBatch();
    } else if (isMod && e.shiftKey && e.key.toLowerCase() === "d") {
      e.preventDefault();
      dryRun = !dryRun;
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (show) handleModalKeydown(e); }} />

{#if show}
  <ModalShell
    open={show}
    title="Multi-Host Command Execution"
    onClose={onClose}
    width={isFullscreen ? "full" : "lg"}
    overlayStyle={isFullscreen ? "" : "padding: 20px"}
    panelClass="w-[94vw] max-w-6xl h-[86vh] max-h-[900px] flex flex-col overflow-hidden text-primary bg-surface border border-border rounded-2xl shadow-2xl transition-all duration-150"
  >
    <!-- Modal Header -->
    <header class="px-5 py-3.5 flex items-center justify-between border-b border-border bg-surface-subtle/50 shrink-0 select-none">
      <div class="flex items-center gap-3">
        <div class="p-2 rounded-xl bg-accent/10 text-accent border border-accent/20">
          <Terminal size={18} />
        </div>
        <div>
          <div class="flex items-center gap-2">
            <h2 class="text-sm font-bold text-primary tracking-tight">Batch Command Runner</h2>
            <span class="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-surface-elevated text-muted border border-border">
              {targetedConnections.length} target{targetedConnections.length === 1 ? "" : "s"}
            </span>
          </div>
          <p class="text-[11px] text-muted leading-tight">Parallel orchestration across remote SSH hosts with dry-run protection</p>
        </div>
      </div>

      <!-- Navigation Tabs -->
      <nav class="flex items-center gap-1 p-1 rounded-xl bg-surface-elevated border border-border">
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1 rounded-lg text-xs font-semibold transition-all {activeTab === 'execute' ? 'bg-surface text-primary shadow-xs' : 'text-muted hover:text-primary'}"
          onclick={() => (activeTab = "execute")}
        >
          <Play size={12} />
          <span>Runner</span>
        </button>

        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1 rounded-lg text-xs font-semibold transition-all {activeTab === 'history' ? 'bg-surface text-primary shadow-xs' : 'text-muted hover:text-primary'}"
          onclick={() => (activeTab = "history")}
        >
          <Clock size={12} />
          <span>History</span>
          {#if history.length > 0}
            <span class="ml-0.5 px-1.5 py-0.2 rounded-full text-[10px] bg-accent/20 text-accent">{history.length}</span>
          {/if}
        </button>

        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1 rounded-lg text-xs font-semibold transition-all {activeTab === 'templates' ? 'bg-surface text-primary shadow-xs' : 'text-muted hover:text-primary'}"
          onclick={() => (activeTab = "templates")}
        >
          <Bookmark size={12} />
          <span>Templates</span>
        </button>
      </nav>

      <!-- Window Actions -->
      <div class="flex items-center gap-1">
        <button
          type="button"
          class="p-1.5 rounded-lg text-muted hover:text-primary hover:bg-surface-elevated transition-colors"
          title={isFullscreen ? "Restore window" : "Maximize window"}
          onclick={() => (isFullscreen = !isFullscreen)}
        >
          {#if isFullscreen}
            <Minimize2 size={15} />
          {:else}
            <Maximize2 size={15} />
          {/if}
        </button>
        <button
          type="button"
          class="p-1.5 rounded-lg text-muted hover:text-primary hover:bg-surface-elevated transition-colors"
          onclick={onClose}
          title="Close dialog (Esc)"
        >
          <X size={15} />
        </button>
      </div>
    </header>

    <!-- Env Warning Alert -->
    {#if showEnvWarning}
      <div class="mx-5 mt-3 px-3.5 py-2.5 rounded-xl border border-warning/30 bg-warning/10 flex items-center justify-between text-xs text-amber-300 shrink-0">
        <div class="flex items-center gap-2.5 min-w-0">
          <TriangleAlert size={16} class="shrink-0 text-warning" />
          <div class="truncate">
            <span class="font-bold mr-1">SSH Environment Warning:</span>
            <span class="opacity-90">{envStatus?.warnings[0]}</span>
          </div>
        </div>
        <button
          type="button"
          class="p-1 rounded text-warning hover:bg-warning/20 transition-colors shrink-0"
          onclick={() => (envWarningDismissed = true)}
          title="Dismiss warning"
        >
          <X size={13} />
        </button>
      </div>
    {/if}

    <!-- ── TAB: EXECUTE (MAIN RUNNER) ──────────────────────────────────────── -->
    {#if activeTab === "execute"}
      <div class="flex flex-1 min-h-0 divide-x divide-border overflow-hidden">
        <!-- LEFT COLUMN: Server Selector Deck -->
        <aside class="w-60 md:w-72 shrink-0 flex flex-col min-h-0 bg-surface-subtle/30">
          <div class="p-3 border-b border-border space-y-2.5">
            <div class="flex items-center justify-between">
              <span class="text-[11px] font-bold text-muted uppercase tracking-wider">Target Hosts</span>
              <span class="text-[10px] font-mono font-semibold px-2 py-0.5 rounded-full bg-accent/15 text-accent border border-accent/25">
                {selectedIds.length} of {connections.length}
              </span>
            </div>

            <div class="relative">
              <Search size={13} class="absolute left-2.5 top-2.5 text-muted pointer-events-none" />
              <input
                type="text"
                bind:value={hostFilter}
                placeholder="Filter by name, host, tag..."
                class="w-full pl-8 pr-2.5 py-1.5 rounded-lg bg-surface-input border border-border text-xs text-primary focus:border-accent focus:outline-none placeholder:text-muted/60"
              />
            </div>

            <!-- Quick Filter Pills -->
            <div class="flex items-center gap-1 flex-wrap text-[10px]">
              <button type="button" class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary transition-colors cursor-pointer" onclick={selectAll}>All</button>
              <button type="button" class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary transition-colors cursor-pointer" onclick={selectNone}>None</button>
              <button type="button" class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary transition-colors cursor-pointer" onclick={selectInvert}>Invert</button>
              <button type="button" class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary transition-colors cursor-pointer" onclick={selectProdOnly}>Prod</button>
              <button type="button" class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary transition-colors cursor-pointer" onclick={selectNonProd}>Non-Prod</button>
            </div>
          </div>

          <!-- Server List -->
          <div class="flex-1 min-h-0 overflow-y-auto p-2 space-y-1">
            {#each filteredConnections as conn (conn.id)}
              {@const isSelected = selectedIds.includes(conn.id)}
              {@const isProd = conn.name.toLowerCase().includes("prod") || conn.tags.some((t) => t.toLowerCase().includes("prod"))}
              <button
                type="button"
                class="flex items-center justify-between w-full px-2.5 py-2 rounded-xl text-left text-xs transition-all border cursor-pointer select-none
                  {isSelected ? 'bg-accent/10 border-accent/30 text-primary font-medium' : 'bg-transparent border-transparent text-muted hover:bg-surface-elevated hover:text-primary'}"
                onclick={() => toggleSelect(conn.id)}
              >
                <div class="flex items-center gap-2.5 min-w-0 truncate">
                  <span class="w-3.5 h-3.5 rounded border flex items-center justify-center shrink-0 transition-colors
                    {isSelected ? 'bg-accent border-accent text-white' : 'border-border bg-surface'}">
                    {#if isSelected}
                      <Check size={10} strokeWidth={3} />
                    {/if}
                  </span>
                  <div class="min-w-0 truncate">
                    <div class="truncate font-medium">{conn.name}</div>
                    <div class="text-[10px] font-mono text-muted truncate">{conn.user}@{conn.host}</div>
                  </div>
                </div>

                {#if isProd}
                  <span class="ml-1 px-1.5 py-0.5 rounded text-[9px] font-mono font-bold bg-amber-500/15 text-amber-400 border border-amber-500/30 uppercase shrink-0">
                    PROD
                  </span>
                {/if}
              </button>
            {:else}
              <div class="p-6 text-center text-xs text-muted">No servers match query</div>
            {/each}
          </div>

          <!-- Bottom Safety Config -->
          <div class="p-3 border-t border-border bg-surface-elevated/40 space-y-2.5">
            <label class="flex items-start gap-2.5 text-xs text-primary cursor-pointer select-none p-2 rounded-xl bg-surface border border-border hover:border-border-strong transition-colors">
              <input type="checkbox" bind:checked={dryRun} class="mt-0.5 rounded border-border text-accent focus:ring-accent" />
              <div>
                <span class="font-bold text-[11px] block">Dry Run Mode</span>
                <span class="text-[10px] text-muted leading-tight block">Simulate execution without sending command to remote shells</span>
              </div>
            </label>

            <div class="flex items-center justify-between gap-2 px-1">
              <label for="batch-timeout" class="text-[11px] font-medium text-muted">Timeout (sec)</label>
              <input
                id="batch-timeout"
                type="number"
                min="1"
                max="300"
                bind:value={timeoutSecs}
                class="w-20 px-2 py-1 rounded-lg bg-surface border border-border text-xs font-mono text-center text-primary focus:border-accent focus:outline-none"
              />
            </div>
          </div>
        </aside>

        <!-- RIGHT COLUMN: Command & Results Deck -->
        <main class="flex-1 flex flex-col min-h-0 p-4 overflow-hidden space-y-3">
          <!-- Production Warning Alert -->
          {#if hasProductionTarget && !dryRun}
            <div class="px-3.5 py-2.5 rounded-xl bg-amber-500/10 border border-amber-500/30 flex items-center gap-2.5 text-amber-300 text-xs shrink-0">
              <ShieldAlert size={18} class="shrink-0 text-amber-400" />
              <div class="flex-1 min-w-0">
                <span class="font-bold block text-amber-200">Production Servers Targeted</span>
                <span class="opacity-90 leading-tight">Live commands will execute immediately on production nodes. Review syntax carefully.</span>
              </div>
            </div>
          {/if}

          <!-- Command Editor -->
          <div class="space-y-1.5 shrink-0">
            <div class="flex items-center justify-between">
              <label for="exec-command-input" class="text-[10px] font-bold text-muted uppercase tracking-wider">Remote Command</label>
              <span class="text-[10px] text-muted">Press <kbd class="px-1 py-0.5 rounded bg-surface-elevated border border-border font-mono">Ctrl+Enter</kbd> to execute</span>
            </div>

            <div class="relative">
              <input
                id="exec-command-input"
                type="text"
                bind:value={command}
                placeholder="e.g. uptime, systemctl status nginx, df -h"
                class="w-full px-3.5 py-2.5 rounded-xl bg-surface-input border border-border text-sm font-mono text-primary focus:border-accent focus:outline-none shadow-inner"
                onkeydown={(e) => { if (e.key === "Enter" && !running) void executeBatch(); }}
              />
            </div>

            <!-- Quick Presets -->
            <div class="flex items-center gap-1.5 flex-wrap pt-0.5">
              <span class="text-[10px] font-medium text-muted mr-1">Presets:</span>
              {#each ["uptime", "df -h", "free -m", "w", "docker ps", "systemctl status"] as preset}
                <button
                  type="button"
                  class="px-2 py-0.5 rounded-lg bg-surface-elevated border border-border text-[10px] font-mono text-muted hover:text-accent hover:border-accent/40 transition-colors cursor-pointer"
                  onclick={() => (command = preset)}
                >
                  {preset}
                </button>
              {/each}
              <button
                type="button"
                class="ml-auto text-[10px] text-accent hover:underline flex items-center gap-1 cursor-pointer"
                onclick={() => {
                  if (command.trim()) {
                    newTemplateCmd = command.trim();
                    newTemplateName = command.trim().split(" ")[0] || "Custom";
                    activeTab = "templates";
                  }
                }}
              >
                <Plus size={10} />
                <span>Save Template</span>
              </button>
            </div>
          </div>

          <!-- Action & Export Bar -->
          <div class="flex items-center justify-between pt-1 border-t border-border shrink-0">
            <div class="flex items-center gap-2">
              <span class="text-xs text-muted">
                Executing on <strong class="text-primary">{selectedIds.length}</strong> server{selectedIds.length === 1 ? "" : "s"}
              </span>
            </div>

            <div class="flex items-center gap-2">
              {#if results && results.length > 0 && !running}
                <div class="flex items-center gap-1">
                  <select
                    bind:value={exportFormat}
                    class="px-2.5 py-1.5 rounded-xl bg-surface-input border border-border text-xs text-primary focus:border-accent focus:outline-none cursor-pointer"
                  >
                    <option value="md">Markdown</option>
                    <option value="csv">CSV</option>
                    <option value="json">JSON</option>
                    <option value="txt">Plain Text</option>
                  </select>
                  <button
                    type="button"
                    class="px-3 py-1.5 rounded-xl bg-surface-elevated border border-border text-xs text-primary hover:border-border-strong flex items-center gap-1.5 transition-colors cursor-pointer"
                    onclick={exportResults}
                    title="Download results"
                  >
                    <Download size={13} />
                    <span>Export</span>
                  </button>
                </div>
              {/if}

              <button
                type="button"
                class="flex items-center gap-2 px-4 py-1.5 rounded-xl text-xs font-semibold shadow-sm transition-all cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed
                  {dryRun ? 'bg-surface-elevated hover:bg-surface-active text-primary border border-border' : hasProductionTarget ? 'bg-amber-600 hover:bg-amber-500 text-white' : 'bg-accent hover:bg-accent-hover text-white'}"
                onclick={executeBatch}
                disabled={running || selectedIds.length === 0}
              >
                {#if running}
                  <RefreshCw size={13} class="animate-spin" />
                  <span>Executing ({selectedIds.length})...</span>
                {:else}
                  <Play size={13} />
                  <span>{dryRun ? "Preview Dry Run" : "Execute Command"}</span>
                {/if}
              </button>
            </div>
          </div>

          <!-- Results View -->
          {#if results}
            <div class="flex-1 flex min-h-0 border border-border rounded-2xl overflow-hidden bg-surface-terminal">
              <!-- Result Hosts Sidebar -->
              <div class="w-56 shrink-0 border-r border-border overflow-y-auto p-1.5 space-y-1 bg-surface-subtle/40">
                {#each results as res, idx (res.connection_id)}
                  <button
                    type="button"
                    class="flex items-center justify-between w-full px-2.5 py-2 rounded-xl text-left text-xs transition-all border cursor-pointer
                      {activeResultIndex === idx ? 'bg-surface-elevated text-primary border-border-strong font-semibold shadow-xs' : 'text-muted border-transparent hover:text-primary hover:bg-surface-subtle'}"
                    onclick={() => (activeResultIndex = idx)}
                  >
                    <div class="truncate min-w-0">
                      <div class="truncate font-medium">{res.name}</div>
                      <div class="text-[10px] font-mono opacity-70 truncate">{res.duration_ms}ms · exit {res.exit_code}</div>
                    </div>
                    {#if res.success}
                      <CheckCircle2 size={14} class="text-emerald-400 shrink-0 ml-1.5" />
                    {:else}
                      <XCircle size={14} class="text-rose-400 shrink-0 ml-1.5" />
                    {/if}
                  </button>
                {/each}
              </div>

              <!-- Output Inspector -->
              {#if results[activeResultIndex]}
                {@const active = results[activeResultIndex]}
                <div class="flex-1 flex flex-col min-h-0 p-3.5 overflow-hidden bg-surface-terminal">
                  <div class="flex items-center justify-between pb-2.5 mb-2.5 border-b border-border/80 text-xs shrink-0">
                    <div class="flex items-center gap-2.5 min-w-0">
                      <span class="font-bold text-primary truncate">{active.name}</span>
                      <span class="font-mono text-muted text-[11px]">{active.user}@{active.host}</span>
                      <span class="px-2 py-0.5 rounded text-[10px] font-mono {active.success ? 'bg-emerald-500/15 text-emerald-400 border border-emerald-500/30' : 'bg-rose-500/15 text-rose-400 border border-rose-500/30'}">
                        exit {active.exit_code} · {active.duration_ms}ms
                      </span>
                    </div>

                    <div class="flex items-center gap-2">
                      <!-- Output View Selector -->
                      <div class="flex items-center rounded-lg bg-surface-elevated p-0.5 border border-border text-[10px]">
                        <button
                          type="button"
                          class="px-2 py-0.5 rounded transition-colors {outputTab === 'all' ? 'bg-surface text-primary font-semibold' : 'text-muted hover:text-primary'}"
                          onclick={() => (outputTab = "all")}
                        >
                          All
                        </button>
                        <button
                          type="button"
                          class="px-2 py-0.5 rounded transition-colors {outputTab === 'stdout' ? 'bg-surface text-primary font-semibold' : 'text-muted hover:text-primary'}"
                          onclick={() => (outputTab = "stdout")}
                        >
                          Stdout
                        </button>
                        <button
                          type="button"
                          class="px-2 py-0.5 rounded transition-colors {outputTab === 'stderr' ? 'bg-surface text-primary font-semibold' : 'text-muted hover:text-primary'}"
                          onclick={() => (outputTab = "stderr")}
                        >
                          Stderr
                        </button>
                      </div>

                      <button
                        type="button"
                        class="p-1.5 rounded-lg bg-surface-elevated border border-border text-muted hover:text-primary transition-colors cursor-pointer"
                        onclick={copyActiveOutput}
                        title="Copy host output"
                      >
                        {#if copied}
                          <Check size={13} class="text-emerald-400" />
                        {:else}
                          <Copy size={13} />
                        {/if}
                      </button>
                    </div>
                  </div>

                  <!-- Terminal Pre Code Display -->
                  <div class="flex-1 min-h-0 overflow-y-auto font-mono text-xs p-3.5 rounded-xl bg-black/40 text-primary border border-border/80 space-y-3">
                    {#if (outputTab === "all" || outputTab === "stdout") && active.stdout}
                      <div>
                        {#if outputTab === "all" && active.stderr}
                          <span class="text-muted text-[9px] uppercase block mb-1 font-sans tracking-widest">stdout</span>
                        {/if}
                        <pre class="m-0 whitespace-pre-wrap font-mono leading-relaxed selection:bg-accent/30">{active.stdout}</pre>
                      </div>
                    {/if}

                    {#if (outputTab === "all" || outputTab === "stderr") && active.stderr}
                      <div class="{outputTab === 'all' && active.stdout ? 'border-t border-border/60 pt-2.5 mt-2.5' : ''}">
                        {#if outputTab === "all"}
                          <span class="text-rose-400 text-[9px] uppercase block mb-1 font-sans tracking-widest">stderr</span>
                        {/if}
                        <pre class="m-0 whitespace-pre-wrap font-mono text-rose-300 leading-relaxed selection:bg-rose-500/30">{active.stderr}</pre>
                      </div>
                    {/if}

                    {#if !active.stdout && !active.stderr}
                      <div class="h-full flex items-center justify-center text-muted italic text-center p-6">
                        No output returned from remote process (exit {active.exit_code})
                      </div>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
          {:else if !running}
            <!-- Empty Ready State -->
            <div class="flex-1 flex flex-col items-center justify-center gap-3 text-center p-8 border border-dashed border-border rounded-2xl bg-surface-subtle/20">
              <div class="p-3 rounded-2xl bg-surface-elevated text-muted border border-border">
                <Terminal size={28} />
              </div>
              <div>
                <p class="text-sm font-semibold text-primary mb-1">Command Ready for Dispatch</p>
                <p class="text-xs text-muted max-w-sm leading-relaxed">
                  Select target nodes on the left deck, specify your command, and click <strong>Preview Dry Run</strong> to inspect before executing.
                </p>
              </div>
            </div>
          {:else}
            <!-- Running State -->
            <div class="flex-1 flex flex-col items-center justify-center gap-3 text-center p-8 border border-border rounded-2xl bg-surface-subtle/20">
              <RefreshCw size={28} class="text-accent animate-spin" />
              <p class="text-sm font-semibold text-primary">Dispatching to {selectedIds.length} node(s)...</p>
              <p class="text-xs text-muted">Running parallel remote sessions with {timeoutSecs}s timeout</p>
            </div>
          {/if}
        </main>
      </div>

    <!-- ── TAB: HISTORY ────────────────────────────────────────────────────── -->
    {:else if activeTab === "history"}
      <div class="flex-1 flex flex-col min-h-0 p-5 overflow-hidden">
        <div class="flex items-center justify-between pb-3 border-b border-border shrink-0">
          <div>
            <h3 class="text-sm font-bold text-primary">Execution Audit Log</h3>
            <p class="text-xs text-muted">Recent multi-host batches executed in this workspace</p>
          </div>
          {#if history.length > 0}
            <button
              type="button"
              class="px-3 py-1.5 rounded-xl bg-surface-elevated border border-border text-xs text-rose-400 hover:border-rose-500/40 flex items-center gap-1.5 transition-colors cursor-pointer"
              onclick={clearHistory}
            >
              <Trash2 size={13} />
              <span>Clear History</span>
            </button>
          {/if}
        </div>

        <div class="flex-1 min-h-0 overflow-y-auto mt-3 space-y-2">
          {#each history as item (item.id)}
            {@const successCount = item.results?.filter((r) => r.success).length ?? 0}
            <div class="p-3.5 rounded-xl border border-border bg-surface-elevated/40 hover:border-border-strong transition-all flex items-center justify-between gap-4">
              <div class="min-w-0 flex-1 space-y-1">
                <div class="flex items-center gap-2">
                  <span class="font-mono text-xs text-primary font-semibold truncate">{item.command}</span>
                  <span class="px-2 py-0.5 rounded text-[9px] font-mono uppercase {item.dryRun ? 'bg-accent/15 text-accent border border-accent/25' : 'bg-emerald-500/15 text-emerald-400 border border-emerald-500/25'}">
                    {item.dryRun ? "Dry Run" : "Live Exec"}
                  </span>
                </div>
                <div class="flex items-center gap-3 text-[11px] text-muted font-mono">
                  <span>{new Date(item.timestamp).toLocaleString()}</span>
                  <span>·</span>
                  <span>{item.targetCount} targets ({successCount}/{item.targetCount} passed)</span>
                  <span>·</span>
                  <span>{item.durationMs}ms</span>
                </div>
              </div>

              <div class="flex items-center gap-2 shrink-0">
                <button
                  type="button"
                  class="px-3 py-1.5 rounded-lg bg-surface border border-border text-xs text-primary hover:border-accent hover:text-accent transition-colors cursor-pointer"
                  onclick={() => {
                    command = item.command;
                    dryRun = item.dryRun;
                    results = item.results;
                    activeTab = "execute";
                    notify("Loaded run results into inspector", "info");
                  }}
                >
                  Inspect
                </button>
                <button
                  type="button"
                  class="px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover transition-colors cursor-pointer"
                  onclick={() => {
                    command = item.command;
                    dryRun = item.dryRun;
                    activeTab = "execute";
                    void executeBatch();
                  }}
                >
                  Run Again
                </button>
              </div>
            </div>
          {:else}
            <div class="h-64 flex flex-col items-center justify-center gap-2 text-center text-muted">
              <Clock size={28} class="opacity-50" />
              <p class="text-xs">No batch executions recorded yet</p>
            </div>
          {/each}
        </div>
      </div>

    <!-- ── TAB: TEMPLATES ──────────────────────────────────────────────────── -->
    {:else if activeTab === "templates"}
      <div class="flex-1 flex flex-col min-h-0 p-5 overflow-hidden">
        <div class="flex items-center justify-between pb-3 border-b border-border shrink-0">
          <div>
            <h3 class="text-sm font-bold text-primary">Command Templates &amp; Runbooks</h3>
            <p class="text-xs text-muted">Store reusable diagnostic commands and operational checklists</p>
          </div>
        </div>

        <!-- Add Template Bar -->
        <div class="mt-3 p-3 rounded-xl bg-surface-elevated border border-border flex items-center gap-2 shrink-0">
          <input
            type="text"
            bind:value={newTemplateName}
            placeholder="Template name (e.g. Health Check)"
            class="w-48 px-3 py-1.5 rounded-lg bg-surface-input border border-border text-xs text-primary focus:border-accent focus:outline-none"
          />
          <input
            type="text"
            bind:value={newTemplateCmd}
            placeholder="Command string (e.g. curl -I https://localhost:443)"
            class="flex-1 px-3 py-1.5 rounded-lg bg-surface-input border border-border text-xs font-mono text-primary focus:border-accent focus:outline-none"
          />
          <button
            type="button"
            class="px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-semibold hover:bg-accent-hover transition-colors cursor-pointer flex items-center gap-1.5"
            onclick={addTemplate}
            disabled={!newTemplateName.trim() || !newTemplateCmd.trim()}
          >
            <Plus size={13} />
            <span>Add Template</span>
          </button>
        </div>

        <!-- Templates Grid -->
        <div class="flex-1 min-h-0 overflow-y-auto mt-3 grid grid-cols-2 gap-2.5">
          {#each templates as tmpl (tmpl.id)}
            {@const isBuiltin = DEFAULT_TEMPLATES.some((d) => d.id === tmpl.id)}
            <div class="p-3 rounded-xl border border-border bg-surface-subtle/30 hover:border-border-strong transition-all flex flex-col justify-between gap-2">
              <div>
                <div class="flex items-center justify-between gap-2 mb-1">
                  <span class="text-xs font-bold text-primary">{tmpl.name}</span>
                  <span class="text-[9px] font-mono px-1.5 py-0.2 rounded {isBuiltin ? 'bg-surface-elevated text-muted' : 'bg-accent/15 text-accent'}">
                    {isBuiltin ? "Built-in" : "Custom"}
                  </span>
                </div>
                <pre class="m-0 text-[11px] font-mono text-muted bg-surface-input/80 p-2 rounded-lg border border-border/60 overflow-x-auto select-all">{tmpl.command}</pre>
              </div>

              <div class="flex items-center justify-end gap-2 pt-1">
                {#if !isBuiltin}
                  <button
                    type="button"
                    class="p-1 rounded text-muted hover:text-rose-400 hover:bg-surface-elevated transition-colors cursor-pointer"
                    title="Delete template"
                    onclick={() => deleteTemplate(tmpl.id)}
                  >
                    <Trash2 size={13} />
                  </button>
                {/if}
                <button
                  type="button"
                  class="px-3 py-1 rounded-lg bg-surface-elevated border border-border text-xs text-primary hover:border-accent hover:text-accent transition-colors cursor-pointer"
                  onclick={() => applyTemplate(tmpl.command)}
                >
                  Use Template
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </ModalShell>
{/if}
