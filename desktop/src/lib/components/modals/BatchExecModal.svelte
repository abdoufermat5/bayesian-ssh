<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    X,
    Play,
    CheckCircle2,
    XCircle,
    Terminal,
    ShieldAlert,
    Copy,
    Check,
    RefreshCw,
    Maximize2,
    Minimize2,
    AlertCircle,
    TriangleAlert,
    ChevronDown,
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

  interface Props {
    show: boolean;
    connections: Connection[];
    onClose: () => void;
  }

  let { show, connections, onClose }: Props = $props();

  // ─── selection state ───────────────────────────────────────────────────────
  let selectedIds = $state<string[]>([]);
  let hostFilter = $state<string>("");
  let command = $state<string>("uptime");
  let dryRun = $state<boolean>(true);
  let timeoutSecs = $state<number>(10);
  let running = $state<boolean>(false);
  let results = $state<BatchExecHostResult[] | null>(null);
  let activeResultIndex = $state<number>(0);
  let copied = $state<boolean>(false);
  let initialized = $state<boolean>(false);

  // ─── resize / fullscreen state ─────────────────────────────────────────────
  let isFullscreen = $state<boolean>(false);
  let modalWidth = $state<number>(900);
  let modalHeight = $state<number>(600);
  let isResizing = $state<boolean>(false);
  let resizeStartX = $state<number>(0);
  let resizeStartY = $state<number>(0);
  let resizeStartW = $state<number>(0);
  let resizeStartH = $state<number>(0);

  // ─── env warning state ─────────────────────────────────────────────────────
  let envStatus = $state<EnvStatus | null>(null);
  let envWarningDismissed = $state<boolean>(false);

  $effect(() => {
    if (show && !initialized) {
      selectedIds = connections.map((c) => c.id);
      initialized = true;
      // Fetch env status once when modal opens
      invoke<EnvStatus>("get_env_status")
        .then((s) => (envStatus = s))
        .catch(() => {});
    } else if (!show) {
      initialized = false;
      results = null;
      envWarningDismissed = false;
    }
  });

  const filteredConnections = $derived(
    connections.filter((c) => {
      if (!hostFilter.trim()) return true;
      const q = hostFilter.toLowerCase().trim();
      return (
        c.name.toLowerCase().includes(q) ||
        c.host.toLowerCase().includes(q) ||
        c.user.toLowerCase().includes(q) ||
        c.tags.some((t) => t.toLowerCase().includes(q))
      );
    })
  );

  const targetedConnections = $derived(
    connections.filter((c) => selectedIds.includes(c.id))
  );

  const hasProductionTarget = $derived(
    targetedConnections.some(
      (c) =>
        c.name.toLowerCase().includes("prod") ||
        c.tags.some((t) => t.toLowerCase().includes("prod"))
    )
  );

  const showEnvWarning = $derived(
    !envWarningDismissed &&
      envStatus !== null &&
      envStatus.warnings.length > 0
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

  function selectTag(tagSubstring: string) {
    selectedIds = connections
      .filter((c) =>
        c.tags.some((t) => t.toLowerCase().includes(tagSubstring.toLowerCase()))
      )
      .map((c) => c.id);
  }

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
    try {
      const res = await invoke<BatchExecHostResult[]>("run_batch_command", {
        connectionIds: selectedIds,
        command: command.trim(),
        dryRun,
        timeoutSecs: Number(timeoutSecs),
      });
      results = res;
      activeResultIndex = 0;
      notify(
        dryRun
          ? `Dry-run preview ready for ${res.length} host(s).`
          : `Executed command on ${res.length} host(s).`,
        "success"
      );
    } catch (err) {
      notify(`Batch execution error: ${err}`, "error");
    } finally {
      running = false;
    }
  }

  function copyActiveOutput() {
    if (!results || !results[activeResultIndex]) return;
    const active = results[activeResultIndex];
    const text = `Host: ${active.name} (${active.user}@${active.host})\nExit Code: ${active.exit_code}\nSTDOUT:\n${active.stdout}\nSTDERR:\n${active.stderr}`;
    navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  // ─── resize drag handlers ──────────────────────────────────────────────────
  function startResize(e: MouseEvent) {
    if (isFullscreen) return;
    isResizing = true;
    resizeStartX = e.clientX;
    resizeStartY = e.clientY;
    resizeStartW = modalWidth;
    resizeStartH = modalHeight;
    e.preventDefault();
  }

  function onMouseMove(e: MouseEvent) {
    if (!isResizing) return;
    const dx = e.clientX - resizeStartX;
    const dy = e.clientY - resizeStartY;
    modalWidth = Math.max(680, Math.min(window.innerWidth - 40, resizeStartW + dx));
    modalHeight = Math.max(440, Math.min(window.innerHeight - 40, resizeStartH + dy));
  }

  function stopResize() {
    isResizing = false;
  }
</script>

<svelte:window onmousemove={onMouseMove} onmouseup={stopResize} />

{#if show}
  <ModalShell
    open={show}
    title="Safe Multi-Host Batch Execution"
    onClose={onClose}
    width={isFullscreen ? "full" : "lg"}
    overlayStyle={isFullscreen ? "" : "padding: 16px"}
    panelStyle={isFullscreen
      ? "width:100vw;height:100vh;border-radius:0;max-width:100%;max-height:100%"
      : `width:${modalWidth}px;height:${modalHeight}px;max-width:calc(100vw - 32px);max-height:calc(100vh - 32px)`}
    panelClass="overflow-hidden text-primary transition-[width,height] duration-150"
  >
      <!-- ── HEADER ─────────────────────────────────────────────────────── -->
      <div class="px-5 py-3 flex items-center justify-between border-b border-border bg-surface-input/30 shrink-0 select-none">
        <div class="flex items-center gap-2.5">
          <Terminal class="text-accent" size={18} />
          <div>
            <h3 class="text-sm font-bold text-primary m-0 leading-tight">Safe Multi-Host Batch Execution</h3>
            <p class="text-[11px] text-muted m-0 leading-tight">Execute remote commands with dry-run preview &amp; production warnings.</p>
          </div>
        </div>
        <div class="flex items-center gap-1">
          <button
            class="p-1.5 rounded-lg text-muted hover:text-primary hover:bg-white/5 transition-all cursor-pointer"
            title={isFullscreen ? "Restore" : "Maximize"}
            onclick={() => (isFullscreen = !isFullscreen)}
          >
            {#if isFullscreen}
              <Minimize2 size={15} />
            {:else}
              <Maximize2 size={15} />
            {/if}
          </button>
          <button
            class="p-1.5 rounded-lg text-muted hover:text-primary hover:bg-white/5 transition-all cursor-pointer"
            onclick={onClose}
            title="Close"
          >
            <X size={15} />
          </button>
        </div>
      </div>

      <!-- ── ENV WARNING BANNER (dismissible) ──────────────────────────── -->
      {#if showEnvWarning}
        <div class="mx-4 mt-3 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2.5 flex items-start gap-2.5 text-xs text-amber-300 shrink-0">
          <TriangleAlert size={16} class="shrink-0 mt-0.5 text-amber-400" />
          <div class="flex-1 min-w-0">
            <span class="font-bold block">SSH Agent Not Detected</span>
            <span class="text-amber-300/80 leading-relaxed">{envStatus?.warnings[0]}</span>
          </div>
          <button
            class="ml-1 p-0.5 rounded text-amber-400 hover:text-amber-200 hover:bg-amber-500/20 transition-all cursor-pointer shrink-0"
            onclick={() => (envWarningDismissed = true)}
            title="Dismiss"
          >
            <X size={13} />
          </button>
        </div>
      {/if}

      <!-- ── BODY ──────────────────────────────────────────────────────── -->
      <div class="flex flex-1 min-h-0 divide-x divide-border overflow-hidden mt-2">

        <!-- LEFT PANEL: host list + safety controls -->
        <div class="w-64 shrink-0 flex flex-col gap-3 p-3 overflow-y-auto bg-surface-input/15">
          <div>
            <div class="flex items-center justify-between mb-2">
              <span class="text-[10px] font-bold text-muted uppercase tracking-wider">Target Hosts</span>
              <span class="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-accent/15 text-accent border border-accent/20">
                {selectedIds.length} / {connections.length}
              </span>
            </div>

            <!-- search -->
            <input
              type="text"
              bind:value={hostFilter}
              placeholder="Filter hosts…"
              class="w-full px-2.5 py-1.5 rounded-lg bg-surface border border-border text-xs text-primary focus:border-accent focus:outline-none mb-2"
            />

            <!-- quick-select buttons -->
            <div class="flex items-center gap-1 mb-2 flex-wrap text-[10px]">
              <button class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary cursor-pointer" onclick={selectAll}>All</button>
              <button class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary cursor-pointer" onclick={selectNone}>Clear</button>
              <button class="px-2 py-0.5 rounded bg-surface border border-border text-muted hover:text-primary cursor-pointer" onclick={selectInvert}>Invert</button>
              <button class="px-2 py-0.5 rounded bg-amber-500/10 border border-amber-500/20 text-amber-400 hover:bg-amber-500/20 cursor-pointer" onclick={() => selectTag("prod")}>Prod</button>
              <button class="px-2 py-0.5 rounded bg-blue-500/10 border border-blue-500/20 text-blue-400 hover:bg-blue-500/20 cursor-pointer" onclick={() => selectTag("staging")}>Staging</button>
            </div>

            <!-- host checkboxes (scrollable, fills remaining space) -->
            <div class="space-y-0.5 overflow-y-auto pr-0.5" style="max-height: 280px">
              {#each filteredConnections as conn}
                {@const isSelected = selectedIds.includes(conn.id)}
                {@const isProd = conn.name.toLowerCase().includes("prod") || conn.tags.some(t => t.toLowerCase().includes("prod"))}
                <button
                  type="button"
                  class="flex items-center justify-between w-full px-2 py-1.5 rounded-lg text-left text-xs transition-all border cursor-pointer select-none
                    {isSelected ? 'bg-accent/10 border-accent/30 text-primary' : 'bg-transparent border-transparent text-muted hover:bg-white/5 hover:text-primary'}"
                  onclick={() => toggleSelect(conn.id)}
                >
                  <div class="flex items-center gap-2 truncate min-w-0">
                    <!-- visual checkbox -->
                    <span class="shrink-0 w-3.5 h-3.5 rounded border flex items-center justify-center transition-all
                      {isSelected ? 'bg-accent border-accent' : 'border-border bg-surface'}">
                      {#if isSelected}
                        <Check size={9} class="text-white" />
                      {/if}
                    </span>
                    <span class="truncate">{conn.name}</span>
                  </div>
                  {#if isProd}
                    <span class="px-1.5 text-[9px] font-extrabold text-amber-400 bg-amber-500/20 border border-amber-500/30 rounded shrink-0 ml-1">PROD</span>
                  {/if}
                </button>
              {:else}
                <div class="p-2 text-center text-xs text-muted italic">No matching hosts</div>
              {/each}
            </div>
          </div>

          <!-- safety controls -->
          <div class="mt-auto pt-3 border-t border-border space-y-2.5">
            <span class="text-[10px] font-bold text-muted uppercase tracking-wider block">Safety Options</span>
            <label class="flex items-center gap-2 text-xs text-primary cursor-pointer p-2 rounded-lg bg-surface border border-border hover:border-border-hover">
              <input type="checkbox" bind:checked={dryRun} class="rounded border-border accent-accent" />
              <div>
                <span class="font-bold block text-[11px]">Dry Run Mode</span>
                <span class="text-[10px] text-muted">Preview without running remote code</span>
              </div>
            </label>
            <div class="space-y-1">
              <label for="batch-exec-timeout" class="text-[10px] font-medium text-muted block">Timeout per host (s)</label>
              <input
                id="batch-exec-timeout"
                type="number" min="1" max="120"
                bind:value={timeoutSecs}
                class="w-full px-2.5 py-1.5 rounded-lg bg-surface border border-border text-xs text-primary focus:border-accent focus:outline-none"
              />
            </div>
          </div>
        </div>

        <!-- RIGHT PANEL: command + results -->
        <div class="flex-1 flex flex-col min-h-0 p-4 overflow-hidden">

          <!-- production warning -->
          {#if hasProductionTarget && !dryRun}
            <div class="mb-3 px-3 py-2.5 rounded-xl bg-amber-500/15 border border-amber-500/30 flex items-center gap-2 text-amber-400 text-xs shrink-0">
              <ShieldAlert size={16} class="shrink-0" />
              <div>
                <span class="font-bold block">⚠️ Production Host(s) Targeted</span>
                <span class="text-amber-300/80">You are executing a live command against production servers. Verify your command before running.</span>
              </div>
            </div>
          {/if}

          <!-- command input -->
          <div class="space-y-2 mb-3 shrink-0">
            <label for="batch-exec-command" class="text-[10px] font-bold text-muted uppercase tracking-wider block">Remote Command</label>
            <input
              id="batch-exec-command"
              type="text"
              bind:value={command}
              placeholder="e.g. uptime, systemctl status nginx, df -h"
              class="w-full px-3 py-2 rounded-xl bg-surface-input border border-border text-sm font-mono text-primary focus:border-accent focus:outline-none shadow-inner"
              onkeydown={(e) => { if (e.key === "Enter" && !running) executeBatch(); }}
            />
            <!-- presets -->
            <div class="flex items-center gap-1.5 flex-wrap">
              <span class="text-[10px] text-muted font-medium">Presets:</span>
              {#each ["uptime", "df -h", "free -m", "w", "docker ps", "systemctl status"] as preset}
                <button
                  class="px-2 py-0.5 rounded-lg bg-surface-input border border-border text-[10px] font-mono text-muted hover:text-accent hover:border-accent/40 transition-all cursor-pointer"
                  onclick={() => (command = preset)}
                >
                  {preset}
                </button>
              {/each}
            </div>
          </div>

          <!-- run button row -->
          <div class="flex items-center justify-between mb-3 shrink-0">
            <span class="text-xs text-muted">Targeting <strong class="text-primary">{selectedIds.length}</strong> server(s)</span>
            <button
              class="flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-bold transition-all shadow cursor-pointer border-none disabled:opacity-50 disabled:cursor-not-allowed
                {dryRun ? 'bg-accent text-white hover:opacity-90' : hasProductionTarget ? 'bg-amber-500 text-black hover:bg-amber-400' : 'bg-emerald-500 text-black hover:bg-emerald-400'}"
              onclick={executeBatch}
              disabled={running}
            >
              {#if running}
                <RefreshCw size={13} class="animate-spin" />
                Executing…
              {:else}
                <Play size={13} />
                {dryRun ? "Preview Dry Run" : "Execute Batch Command"}
              {/if}
            </button>
          </div>

          <!-- results -->
          {#if results}
            <div class="flex-1 flex min-h-0 border border-border rounded-xl overflow-hidden bg-black/40">
              <!-- host tabs sidebar -->
              <div class="w-48 shrink-0 border-r border-border overflow-y-auto p-1.5 space-y-0.5 bg-surface-input/30">
                {#each results as res, idx}
                  <button
                    class="flex items-center justify-between w-full px-2.5 py-2 rounded-lg text-left text-xs transition-all border
                      {activeResultIndex === idx ? 'bg-accent/20 text-accent border-accent/30 font-bold' : 'text-muted border-transparent hover:text-primary hover:bg-white/5'}"
                    onclick={() => (activeResultIndex = idx)}
                  >
                    <div class="truncate min-w-0">
                      <div class="truncate font-medium">{res.name}</div>
                      <div class="text-[10px] opacity-70 font-mono truncate">{res.user}@{res.host}</div>
                    </div>
                    {#if res.success}
                      <CheckCircle2 size={13} class="text-emerald-400 shrink-0 ml-1" />
                    {:else}
                      <XCircle size={13} class="text-rose-400 shrink-0 ml-1" />
                    {/if}
                  </button>
                {/each}
              </div>

              <!-- output inspector -->
              {#if results[activeResultIndex]}
                {@const active = results[activeResultIndex]}
                <div class="flex-1 flex flex-col min-h-0 p-3 overflow-hidden">
                  <div class="flex items-center justify-between pb-2 mb-2 border-b border-border/50 text-xs shrink-0">
                    <div class="flex items-center gap-2 min-w-0 flex-wrap">
                      <span class="font-bold text-primary truncate">{active.name}</span>
                      <span class="font-mono text-muted text-[10px]">{active.user}@{active.host}</span>
                      <span class="px-2 py-0.5 rounded text-[10px] font-mono {active.success ? 'bg-emerald-500/15 text-emerald-400 border border-emerald-500/30' : 'bg-rose-500/15 text-rose-400 border border-rose-500/30'}">
                        exit {active.exit_code} · {active.duration_ms}ms
                      </span>
                    </div>
                    <button
                      class="flex items-center gap-1 px-2 py-1 rounded bg-surface border border-border text-[10px] text-muted hover:text-primary cursor-pointer ml-2 shrink-0"
                      onclick={copyActiveOutput}
                    >
                      {#if copied}
                        <Check size={11} class="text-emerald-400" /> Copied
                      {:else}
                        <Copy size={11} /> Copy
                      {/if}
                    </button>
                  </div>

                  <!-- terminal output -->
                  <div class="flex-1 min-h-0 overflow-y-auto font-mono text-xs p-3 rounded-lg bg-black/70 text-emerald-300 leading-relaxed border border-white/5 space-y-2">
                    {#if active.stdout}
                      <div>
                        <span class="text-muted text-[9px] uppercase block mb-1 font-sans tracking-widest">stdout</span>
                        <pre class="m-0 whitespace-pre-wrap">{active.stdout}</pre>
                      </div>
                    {/if}
                    {#if active.stderr}
                      <div class="text-rose-400 border-t border-white/10 pt-2 mt-2">
                        <span class="text-rose-300 text-[9px] uppercase block mb-1 font-sans tracking-widest">stderr</span>
                        <pre class="m-0 whitespace-pre-wrap">{active.stderr}</pre>
                      </div>
                    {/if}
                    {#if !active.stdout && !active.stderr}
                      <span class="text-muted italic">[No output returned]</span>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
          {:else if !running}
            <!-- empty state -->
            <div class="flex-1 flex flex-col items-center justify-center gap-3 text-center p-8 border border-dashed border-border rounded-xl opacity-60">
              <Terminal size={36} class="text-muted" />
              <div>
                <p class="text-sm font-semibold text-muted m-0">Ready to execute</p>
                <p class="text-xs text-muted m-0">Select hosts, enter a command, then click <strong>Preview Dry Run</strong> first.</p>
              </div>
            </div>
          {:else}
            <!-- running state -->
            <div class="flex-1 flex flex-col items-center justify-center gap-3 text-center">
              <RefreshCw size={32} class="text-accent animate-spin" />
              <p class="text-sm text-muted m-0">Running on {selectedIds.length} host(s)…</p>
            </div>
          {/if}
        </div>
      </div>

      <!-- ── RESIZE HANDLE ──────────────────────────────────────────────── -->
      {#if !isFullscreen}
        <button
          type="button"
          class="absolute bottom-0 right-0 w-5 h-5 cursor-se-resize opacity-30 hover:opacity-70 transition-opacity border-none p-0 bg-transparent"
          onmousedown={startResize}
          onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') startResize(e as unknown as MouseEvent); }}
          aria-label="Resize dialog"
          style="background: linear-gradient(135deg, transparent 50%, var(--color-accent, #6366f1) 50%) !important; border-bottom-right-radius: 1rem;"
        ></button>
      {/if}
  </ModalShell>
{/if}
