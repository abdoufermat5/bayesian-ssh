<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    Ban,
    CircleCheck,
    CircleX,
    Clock,
    File,
    FolderPlus,
    LoaderCircle,
    Maximize2,
    Minimize2,
    RefreshCw,
    Search,
    Trash2,
    TriangleAlert,
    Upload,
    X,
  } from "lucide-svelte";
  import type {
    Connection,
    HostUploadResult,
    UploadEventPayload,
    UploadFinishedPayload,
  } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { appState } from "$lib/stores/appState.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    show: boolean;
    connections: Connection[];
    initialConnectionIds?: string[];
    initialRemoteDir?: string | null;
    onClose: () => void;
    onComplete?: (touchedConnectionIds: string[]) => void;
  }

  let {
    show,
    connections,
    initialConnectionIds = undefined,
    initialRemoteDir = null,
    onClose,
    onComplete,
  }: Props = $props();

  type RowStatus = "queued" | "uploading" | "done" | "failed" | "cancelled";

  interface UploadRow {
    localPath: string;
    connectionId: string;
    name: string;
    host: string;
    status: RowStatus;
    bytesDone: number;
    bytesTotal: number | null;
    /** Bytes per second, measured from the first progress event. */
    speed: number;
    startedAt: number | null;
    viaScp: boolean;
    error: string | null;
    files: number;
    bytes: number;
    remotePath: string | null;
    elapsedMs: number;
  }

  // ─── Modal state ─────────────────────────────────────────────────────────────
  let isFullscreen = $state(false);
  let initialized = $state(false);

  // ─── Sources ─────────────────────────────────────────────────────────────────
  let sources = $state.raw<string[]>([]);
  let dragging = $state(false);

  // ─── Targets ─────────────────────────────────────────────────────────────────
  const selectedIds = new SvelteSet<string>();
  let hostFilter = $state("");
  let tagFilter = $state<string | null>(null);
  let remoteDir = $state("/");

  // ─── Run state ───────────────────────────────────────────────────────────────
  let rows = $state<Record<string, UploadRow>>({});
  let jobPaths = $state.raw<string[]>([]);
  let jobConnectionIds = $state.raw<string[]>([]);
  let running = $state(false);
  let cancelling = $state(false);
  let hasRun = $state(false);
  let jobError = $state<string | null>(null);
  let activeJobId: string | null = null;
  let awaitingJob = false;
  let pendingEvents: UploadEventPayload[] = [];
  let pendingFinished: UploadFinishedPayload | null = null;

  let unlistenEvent: UnlistenFn | null = null;
  let unlistenFinished: UnlistenFn | null = null;

  const rowKey = (localPath: string, connectionId: string) => `${localPath}\u0000${connectionId}`;

  $effect(() => {
    if (show && !initialized) {
      untrack(() => {
        sources = [];
        rows = {};
        jobPaths = [];
        jobConnectionIds = [];
        activeJobId = null;
        awaitingJob = false;
        pendingEvents = [];
        pendingFinished = null;
        hasRun = false;
        running = false;
        cancelling = false;
        jobError = null;
        remoteDir = initialRemoteDir || appState.settings.sftp_default_remote_path || "/";
        // Never preselect hosts implicitly: uploading overwrites remote files.
        replaceSelection(initialConnectionIds ?? []);
      });
      initialized = true;
    } else if (!show) {
      initialized = false;
    }
  });

  // Subscriptions live as long as the modal is open: the backend can emit the
  // first `Started` event before `start_upload` resolves with the job id.
  $effect(() => {
    if (!show) return;
    let disposed = false;

    listen<UploadEventPayload>("upload-event", (e) => handleUploadEvent(e.payload)).then((un) => {
      if (disposed) un();
      else unlistenEvent = un;
    });
    listen<UploadFinishedPayload>("upload-finished", (e) => handleUploadFinished(e.payload)).then(
      (un) => {
        if (disposed) un();
        else unlistenFinished = un;
      },
    );

    // Drag & drop of OS files is delivered by the Tauri webview, not the DOM.
    let unlistenDrop: UnlistenFn | null = null;
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type === "over" || event.payload.type === "enter") {
            dragging = true;
          } else if (event.payload.type === "drop") {
            dragging = false;
            if (!running) addSources(event.payload.paths);
          } else {
            dragging = false;
          }
        })
        .then((un) => {
          if (disposed) un();
          else unlistenDrop = un;
        })
        .catch(() => {});
    } catch {
      // No Tauri webview (browser/e2e mock): the pickers still work.
    }

    return () => {
      disposed = true;
      unlistenEvent?.();
      unlistenFinished?.();
      unlistenDrop?.();
      unlistenEvent = null;
      unlistenFinished = null;
    };
  });

  const prodIds = $derived(
    new Set(
      connections
        .filter(
          (c) =>
            c.name.toLowerCase().includes("prod") ||
            c.tags.some((t) => t.toLowerCase().includes("prod")),
        )
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

  const rowList = $derived(Object.values(rows));
  const failedRows = $derived(rowList.filter((r) => r.status === "failed"));

  /**
   * Per-host rollup. There is one row per (file, host), so counting rows would
   * report "4 hosts" for 2 files × 2 hosts. A host is done once every file
   * reached it, failed if any file failed, cancelled if any file was cancelled.
   */
  const hostSummary = $derived.by(() => {
    const byHost = new Map<string, RowStatus[]>();
    for (const row of rowList) {
      const list = byHost.get(row.connectionId);
      if (list) list.push(row.status);
      else byHost.set(row.connectionId, [row.status]);
    }
    let done = 0;
    let failed = 0;
    let cancelled = 0;
    for (const statuses of byHost.values()) {
      if (statuses.includes("failed")) failed++;
      else if (statuses.includes("cancelled")) cancelled++;
      else if (statuses.every((s) => s === "done")) done++;
    }
    return { total: byHost.size, done, failed, cancelled };
  });

  /** Rows grouped by local path (display order follows the job order). */
  const groups = $derived.by(() => {
    const byPath = new Map<string, UploadRow[]>();
    for (const row of rowList) {
      const list = byPath.get(row.localPath);
      if (list) list.push(row);
      else byPath.set(row.localPath, [row]);
    }
    const connOrder = new Map(jobConnectionIds.map((id, i) => [id, i]));
    const pathOrder = new Map(jobPaths.map((p, i) => [p, i]));
    return Array.from(byPath.entries())
      .map(([path, list]) => ({
        path,
        rows: list.sort(
          (a, b) =>
            (connOrder.get(a.connectionId) ?? Number.MAX_SAFE_INTEGER) -
              (connOrder.get(b.connectionId) ?? Number.MAX_SAFE_INTEGER) ||
            a.name.localeCompare(b.name),
        ),
      }))
      .sort(
        (a, b) =>
          (pathOrder.get(a.path) ?? Number.MAX_SAFE_INTEGER) -
          (pathOrder.get(b.path) ?? Number.MAX_SAFE_INTEGER),
      );
  });

  /** Aggregate progress across rows whose total size is known. */
  const overall = $derived.by(() => {
    let done = 0;
    let total = 0;
    let known = 0;
    for (const row of rowList) {
      done += row.bytesDone;
      if (row.bytesTotal && row.bytesTotal > 0) {
        total += row.bytesTotal;
        known += 1;
      }
    }
    return { done, total, known, pct: total > 0 ? Math.min(100, Math.round((done / total) * 100)) : null };
  });

  // ─── Selection helpers ───────────────────────────────────────────────────────
  function toggleSelect(id: string) {
    if (selectedIds.has(id)) selectedIds.delete(id);
    else selectedIds.add(id);
  }

  function replaceSelection(ids: Iterable<string>) {
    selectedIds.clear();
    for (const id of ids) selectedIds.add(id);
  }

  function selectAll() {
    for (const c of isFiltered ? filteredConnections : connections) selectedIds.add(c.id);
  }

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

  // ─── Sources ─────────────────────────────────────────────────────────────────
  function addSources(paths: string[]) {
    if (paths.length === 0) return;
    const seen = new Set(sources);
    const next = [...sources];
    for (const p of paths) {
      if (p && !seen.has(p)) {
        seen.add(p);
        next.push(p);
      }
    }
    if (next.length !== sources.length) sources = next;
  }

  function removeSource(path: string) {
    sources = sources.filter((p) => p !== path);
  }

  async function pickFiles() {
    try {
      const picked = await invoke<string[]>("pick_upload_files");
      addSources(picked ?? []);
    } catch (err) {
      notify(`File picker failed: ${err}`, "error");
    }
  }

  async function pickFolder() {
    try {
      const picked = await invoke<string | null>("pick_upload_folder");
      if (picked) addSources([picked]);
    } catch (err) {
      notify(`Folder picker failed: ${err}`, "error");
    }
  }

  function onDomDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
  }

  // ─── Run ─────────────────────────────────────────────────────────────────────
  function newRow(localPath: string, connectionId: string): UploadRow {
    const conn = connections.find((c) => c.id === connectionId);
    return {
      localPath,
      connectionId,
      name: conn?.name ?? connectionId,
      host: conn?.host ?? "",
      status: "queued",
      bytesDone: 0,
      bytesTotal: null,
      speed: 0,
      startedAt: null,
      viaScp: false,
      error: null,
      files: 0,
      bytes: 0,
      remotePath: null,
      elapsedMs: 0,
    };
  }

  function upsert(localPath: string, connectionId: string, update: (row: UploadRow) => UploadRow) {
    const key = rowKey(localPath, connectionId);
    const base = rows[key] ?? newRow(localPath, connectionId);
    rows = { ...rows, [key]: update(base) };
  }

  function applyEvent(payload: UploadEventPayload) {
    const { localPath, event } = payload;
    if (event.kind === "started") {
      upsert(localPath, event.connectionId, (row) => ({
        ...row,
        name: event.name,
        host: event.host,
        status: "uploading",
        startedAt: row.startedAt ?? Date.now(),
      }));
    } else if (event.kind === "progress") {
      upsert(localPath, event.connectionId, (row) => {
        const elapsed = row.startedAt ? (Date.now() - row.startedAt) / 1000 : 0;
        return {
          ...row,
          status: "uploading",
          bytesDone: event.bytesDone,
          bytesTotal: event.bytesTotal ?? row.bytesTotal,
          speed: elapsed > 0.2 ? event.bytesDone / elapsed : row.speed,
        };
      });
    } else {
      const result = event.result;
      upsert(localPath, result.connectionId, (row) => ({ ...row, ...fromResult(result) }));
    }
  }

  function fromResult(result: HostUploadResult): Partial<UploadRow> {
    return {
      name: result.name,
      host: result.host,
      status: result.ok ? "done" : "failed",
      error: result.error ?? null,
      files: result.files,
      bytes: result.bytes,
      viaScp: result.viaScp,
      remotePath: result.remotePath,
      elapsedMs: result.elapsedMs,
      bytesDone: result.bytes,
      bytesTotal: result.bytes > 0 ? result.bytes : null,
    };
  }

  function handleUploadEvent(payload: UploadEventPayload) {
    if (payload.jobId !== activeJobId) {
      // Events for a job whose id we have not received yet: buffer them.
      if (awaitingJob) pendingEvents.push(payload);
      return;
    }
    applyEvent(payload);
  }

  function handleUploadFinished(payload: UploadFinishedPayload) {
    if (payload.jobId !== activeJobId) {
      // The job can finish before `start_upload` resolves with its id.
      if (awaitingJob) pendingFinished = payload;
      return;
    }

    const next = { ...rows };
    if (payload.cancelled) {
      for (const [key, row] of Object.entries(next)) {
        if (row.status === "queued" || row.status === "uploading") {
          next[key] = { ...row, status: "cancelled" };
        }
      }
    } else {
      for (const entry of payload.results ?? []) {
        for (const host of entry.hosts) {
          const key = rowKey(entry.localPath, host.connectionId);
          const base = next[key] ?? newRow(entry.localPath, host.connectionId);
          next[key] = { ...base, ...fromResult(host) };
        }
      }
    }
    rows = next;
    running = false;
    cancelling = false;
    hasRun = true;
    activeJobId = null;
    awaitingJob = false;
    pendingEvents = [];
    jobError = payload.error ?? null;

    if (payload.error) {
      notify(`Upload failed: ${payload.error}`, "error");
    } else if (payload.cancelled) {
      notify("Upload cancelled", "info");
    } else {
      const { done, failed } = hostSummary;
      notify(
        failed > 0
          ? `Upload finished: ${done} host${done === 1 ? "" : "s"} succeeded, ${failed} failed`
          : `Upload finished: ${done} host${done === 1 ? "" : "s"} updated`,
        failed > 0 ? "error" : "success",
      );
    }

    const touched = Array.from(
      new Set(Object.values(rows).filter((r) => r.status === "done").map((r) => r.connectionId)),
    );
    onComplete?.(touched);
  }

  async function runUpload(paths: string[], connectionIds: string[], seed = true) {
    running = true;
    cancelling = false;
    jobError = null;
    awaitingJob = true;
    pendingEvents = [];
    pendingFinished = null;
    jobPaths = paths;
    jobConnectionIds = connectionIds;

    if (seed) {
      const next = { ...rows };
      for (const path of paths) {
        for (const id of connectionIds) {
          next[rowKey(path, id)] = newRow(path, id);
        }
      }
      rows = next;
    }

    try {
      const jobId = await invoke<string>("start_upload", {
        connectionIds,
        localPaths: paths,
        remoteDir: remoteDir.trim(),
        parallel: null,
      });
      activeJobId = jobId;
      awaitingJob = false;
      const buffered = pendingEvents;
      pendingEvents = [];
      for (const payload of buffered) applyEvent(payload);
      const bufferedFinished = pendingFinished;
      pendingFinished = null;
      if (bufferedFinished) handleUploadFinished(bufferedFinished);
    } catch (err) {
      awaitingJob = false;
      pendingEvents = [];
      pendingFinished = null;
      running = false;
      jobError = String(err);
      notify(`Upload failed to start: ${err}`, "error");
    }
  }

  async function startUpload() {
    if (sources.length === 0) {
      notify("Add at least one file or folder.", "error");
      return;
    }
    if (selectedIds.size === 0) {
      notify("Select at least one host.", "error");
      return;
    }
    if (!remoteDir.trim()) {
      notify("Enter a destination directory.", "error");
      return;
    }
    await runUpload([...sources], targetedConnections.map((c) => c.id));
  }

  function retryFailed() {
    const failed = Object.values(rows).filter((r) => r.status === "failed");
    if (failed.length === 0) return;
    const paths = Array.from(new Set(failed.map((r) => r.localPath)));
    const connectionIds = Array.from(new Set(failed.map((r) => r.connectionId)));
    const next = { ...rows };
    for (const row of failed) {
      next[rowKey(row.localPath, row.connectionId)] = {
        ...row,
        status: "queued",
        bytesDone: 0,
        speed: 0,
        startedAt: null,
        error: null,
      };
    }
    rows = next;
    void runUpload(paths, connectionIds, false);
  }

  async function cancelUpload() {
    if (!activeJobId || cancelling) return;
    cancelling = true;
    try {
      await invoke("cancel_upload", { jobId: activeJobId });
    } catch (err) {
      cancelling = false;
      notify(`Failed to cancel: ${err}`, "error");
    }
  }

  function resetResults() {
    rows = {};
    jobPaths = [];
    jobConnectionIds = [];
    hasRun = false;
    jobError = null;
  }

  // ─── Presentation helpers ────────────────────────────────────────────────────
  function formatBytes(bytes: number): string {
    if (!bytes || bytes <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
    return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
  }

  function percentOf(row: UploadRow): number | null {
    if (!row.bytesTotal || row.bytesTotal <= 0) return null;
    return Math.min(100, Math.round((row.bytesDone / row.bytesTotal) * 100));
  }

  function statusClass(status: RowStatus): string {
    if (status === "done") return "text-success";
    if (status === "failed") return "text-error";
    if (status === "cancelled") return "text-muted";
    if (status === "uploading") return "text-accent";
    return "text-faint";
  }

  function barClass(status: RowStatus): string {
    if (status === "done") return "bg-success";
    if (status === "failed") return "bg-error";
    if (status === "cancelled") return "bg-faint";
    if (status === "uploading") return "bg-accent";
    return "bg-transparent";
  }

  function statusLabel(status: RowStatus): string {
    if (status === "done") return "Done";
    if (status === "failed") return "Failed";
    if (status === "cancelled") return "Cancelled";
    if (status === "uploading") return "Uploading";
    return "Queued";
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (!show) return;
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && !running) {
      e.preventDefault();
      void startUpload();
    }
  }}
/>

{#snippet statusIcon(row: UploadRow)}
  {#if row.status === "done"}
    <CircleCheck size={15} class="shrink-0 text-success" aria-hidden="true" />
  {:else if row.status === "failed"}
    <CircleX size={15} class="shrink-0 text-error" aria-hidden="true" />
  {:else if row.status === "cancelled"}
    <Ban size={15} class="shrink-0 text-muted" aria-hidden="true" />
  {:else if row.status === "uploading"}
    <LoaderCircle size={15} class="shrink-0 animate-spin text-accent" aria-hidden="true" />
  {:else}
    <Clock size={15} class="shrink-0 text-faint" aria-hidden="true" />
  {/if}
{/snippet}

{#if show}
  <ModalShell
    open={show}
    title="Upload files to hosts"
    {onClose}
    width={isFullscreen ? "full" : "lg"}
    panelStyle={isFullscreen ? "" : "max-width: 1120px; height: min(780px, calc(100dvh - 48px));"}
    overlayStyle={isFullscreen ? "padding: 0" : ""}
  >
    <header class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Upload files to hosts</h2>
        <p class="modal-subtitle">
          Copy files or folders to several servers at once, in parallel.
        </p>
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

    <div class="flex min-h-0 flex-1 overflow-hidden">
      <!-- Host picker -->
      <aside class="flex w-72 shrink-0 flex-col border-r border-border">
        <div class="flex flex-col gap-2 px-4 pb-2 pt-4">
          <div class="flex items-center justify-between">
            <span class="section-label">Hosts</span>
            <span class="text-xs tabular-nums text-muted"
              >{selectedIds.size} of {connections.length} selected</span
            >
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
            <button
              type="button"
              class="btn btn-ghost btn-sm px-1.5"
              onclick={() => replaceSelection(prodIds)}
            >
              Prod
            </button>
            <button
              type="button"
              class="btn btn-ghost btn-sm px-1.5"
              onclick={() =>
                replaceSelection(connections.filter((c) => !prodIds.has(c.id)).map((c) => c.id))}
            >
              Non-prod
            </button>
          </div>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto border-t border-border-subtle px-2 py-1.5">
          {#each filteredConnections as conn (conn.id)}
            <label
              class="flex h-11 items-center gap-2.5 rounded-md px-2 transition-colors duration-fast {running
                ? 'opacity-60'
                : 'cursor-pointer hover:bg-surface-hover'}"
            >
              <input
                type="checkbox"
                class="checkbox"
                checked={selectedIds.has(conn.id)}
                disabled={running}
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

      <!-- Sources, destination and progress -->
      <section class="flex min-w-0 flex-1 flex-col">
        <div class="flex shrink-0 flex-col gap-3 border-b border-border p-4">
          <div class="field">
            <div class="flex items-center justify-between">
              <span class="field-label">Files &amp; folders</span>
              <span class="text-xs tabular-nums text-muted">{sources.length} selected</span>
            </div>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="rounded-md border border-dashed px-3 py-2.5 transition-colors duration-fast {dragging
                ? 'border-accent bg-accent-muted'
                : 'border-border'}"
              ondragover={(e) => {
                e.preventDefault();
                dragging = true;
              }}
              ondragleave={() => (dragging = false)}
              ondrop={onDomDrop}
            >
              {#if sources.length === 0}
                <p class="m-0 py-2 text-center text-xs text-muted">
                  Drag files here, or use the buttons below.
                </p>
              {:else}
                <ul class="m-0 flex flex-col gap-1 p-0">
                  {#each sources as path (path)}
                    <li class="flex items-center gap-2">
                      <File size={14} class="shrink-0 text-muted" />
                      <span class="min-w-0 flex-1 truncate font-mono text-xs" title={path}>{path}</span>
                      <button
                        type="button"
                        class="btn-icon btn-icon-sm"
                        aria-label="Remove {path}"
                        title="Remove"
                        disabled={running}
                        onclick={() => removeSource(path)}
                      >
                        <Trash2 size={14} />
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
            <div class="flex items-center gap-1.5">
              <button type="button" class="btn btn-secondary btn-sm" disabled={running} onclick={pickFiles}>
                <Upload size={14} />
                Add files
              </button>
              <button type="button" class="btn btn-secondary btn-sm" disabled={running} onclick={pickFolder}>
                <FolderPlus size={14} />
                Add folder
              </button>
            </div>
          </div>

          <div class="field">
            <label class="field-label" for="upload-remote-dir">Destination directory</label>
            <input
              id="upload-remote-dir"
              type="text"
              class="input input-mono"
              spellcheck="false"
              autocomplete="off"
              bind:value={remoteDir}
              disabled={running}
              placeholder="/var/www"
            />
            <span class="field-hint">Every file is copied into this directory on each host.</span>
          </div>

          {#if hasProductionTarget}
            <div class="alert alert-warning">
              <TriangleAlert size={14} class="mt-0.5 shrink-0" />
              <span>
                <span class="alert-title">Production hosts selected.</span>
                Files are copied for real on production.
              </span>
            </div>
          {/if}

          <div class="flex items-center gap-2">
            {#if hasRun && !running}
              <button type="button" class="btn btn-ghost btn-sm" onclick={resetResults}>
                <RefreshCw size={14} />
                Reset
              </button>
            {/if}
            <button
              type="button"
              class="btn btn-primary ml-auto"
              onclick={startUpload}
              disabled={running || sources.length === 0 || selectedIds.size === 0}
            >
              {#if running}
                <span class="spinner size-3"></span>
                Uploading…
              {:else}
                <Upload size={14} />
                Upload to {selectedIds.size} host{selectedIds.size === 1 ? "" : "s"}
              {/if}
            </button>
          </div>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto p-4">
          {#if jobError}
            <div class="alert alert-error mb-3">
              <TriangleAlert size={14} class="mt-0.5 shrink-0" />
              <span>{jobError}</span>
            </div>
          {/if}

          {#if running || hasRun}
            <div class="mb-3 flex items-center gap-3">
              <span class="text-sm font-medium text-primary">
                {#if running}Uploading{:else}Results{/if}
              </span>
              <span class="flex items-center gap-1.5 text-xs tabular-nums text-muted">
                <span>{hostSummary.done} of {hostSummary.total} hosts done</span>
                {#if hostSummary.failed > 0}
                  <span aria-hidden="true">·</span>
                  <span class="text-error">{hostSummary.failed} failed</span>
                {/if}
                {#if hostSummary.cancelled > 0}
                  <span aria-hidden="true">·</span>
                  <span>{hostSummary.cancelled} cancelled</span>
                {/if}
              </span>
              <div class="ml-auto flex items-center gap-2">
                {#if running}
                  <button type="button" class="btn btn-danger btn-sm" disabled={cancelling} onclick={cancelUpload}>
                    {#if cancelling}
                      <span class="spinner size-3"></span>
                      Cancelling…
                    {:else}
                      Cancel
                    {/if}
                  </button>
                {:else if failedRows.length > 0}
                  <button type="button" class="btn btn-secondary btn-sm" onclick={retryFailed}>
                    <RefreshCw size={14} />
                    Retry failed
                  </button>
                {/if}
              </div>
            </div>

            {#if running}
              <div class="mb-3 flex items-center gap-3">
                <div
                  class="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-surface-hover"
                  role="progressbar"
                  aria-label="Overall upload progress"
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-valuenow={overall.pct ?? undefined}
                >
                  <div
                    class="h-full rounded-full bg-accent transition-[width] duration-fast"
                    style="width: {overall.pct ?? 30}%"
                  ></div>
                </div>
                <span class="shrink-0 font-mono text-xs tabular-nums text-muted">
                  {formatBytes(overall.done)}{#if overall.known > 0}/{formatBytes(overall.total)}{/if}
                </span>
              </div>
            {/if}

            <div class="table-wrap">
              {#each groups as group (group.path)}
                <div class="border-b border-border-subtle bg-surface-subtle px-4 py-1.5">
                  <span class="truncate font-mono text-xs text-muted" title={group.path}>{group.path}</span>
                </div>
                {#each group.rows as row (row.connectionId)}
                  {@const pct = percentOf(row)}
                  <div class="flex flex-col gap-1.5 px-4 py-2.5">
                    <div class="flex items-center gap-3">
                      {@render statusIcon(row)}
                      <span class="shrink-0 text-sm text-primary">{row.name}</span>
                      <span class="min-w-0 truncate font-mono text-xs text-muted">{row.host}</span>
                      {#if row.viaScp}
                        <span class="badge badge-neutral">via SCP</span>
                      {/if}
                      <span class="ml-auto shrink-0 text-xs {statusClass(row.status)}">
                        {statusLabel(row.status)}
                      </span>
                    </div>

                    {#if row.status !== "queued"}
                      <div class="flex items-center gap-3">
                        <div
                          class="h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-surface-hover"
                          role="progressbar"
                          aria-label="Upload progress for {row.name}"
                          aria-valuemin="0"
                          aria-valuemax="100"
                          aria-valuenow={pct ?? undefined}
                        >
                          <div
                            class="h-full rounded-full {barClass(row.status)} transition-[width] duration-fast"
                            style="width: {row.status === 'done'
                              ? '100%'
                              : pct !== null
                                ? `${pct}%`
                                : row.status === 'uploading'
                                  ? '30%'
                                  : '0%'}"
                          ></div>
                        </div>
                        <span class="shrink-0 font-mono text-xs tabular-nums text-muted">
                          {#if row.status === "uploading"}
                            {formatBytes(row.bytesDone)}{#if row.bytesTotal}/{formatBytes(row.bytesTotal)}{/if}
                            {#if row.speed > 0}· {formatBytes(row.speed)}/s{/if}
                          {:else if row.status === "done"}
                            {formatBytes(row.bytes)}{#if row.files > 0} · {row.files}
                              file{row.files === 1 ? "" : "s"}{/if}
                          {/if}
                        </span>
                      </div>
                    {/if}

                    {#if row.error}
                      <p class="m-0 pl-7 text-xs text-error">{row.error}</p>
                    {/if}
                  </div>
                {/each}
              {/each}
            </div>
          {:else}
            <div class="empty-state py-12">
              <Upload size={22} class="mb-3 text-muted" aria-hidden="true" />
              <div class="empty-state-title">No upload yet</div>
              <div class="empty-state-desc">
                Pick the files to send, choose the destination, then select the hosts.
              </div>
            </div>
          {/if}
        </div>
      </section>
    </div>
  </ModalShell>
{/if}
