<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { FolderOpen, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type {
    AiPruneMode,
    AiPruneOutcome,
    AiPrunePlan,
    AiPruneRequest,
    AiPruneSelection,
    AiPruneWhat,
    AiReportSummary,
    AiStorage,
  } from "$lib/types";
  import { formatDateTime } from "$lib/utils/timezone";
  import { shortId } from "$lib/utils/ai";
  import { notify } from "$lib/stores/notifications.svelte";

  interface Props {
    timezone: string;
    /** Opens with only this run selected (the per-run "Delete…" entry). */
    initialRunId?: string | null;
    onClose: () => void;
    /** Called after a prune ran, so the caller can refresh its lists. */
    onDone: (outcome: AiPruneOutcome) => void;
  }

  let { timezone, initialRunId = null, onClose, onDone }: Props = $props();

  type SelectionKind = "older_than" | "runs" | "all";

  const PRESETS = [7, 30, 90];

  let storage = $state<AiStorage | null>(null);
  let runList = $state.raw<AiReportSummary[]>([]);
  let selectionKind = $state<SelectionKind>(untrack(() => (initialRunId ? "runs" : "older_than")));
  let days = $state(30);
  let picked = $state.raw<string[]>(untrack(() => (initialRunId ? [initialRunId] : [])));
  let mode = $state<AiPruneMode>("delete");
  let archiveDir = $state<string | null>(null);
  let what = $state<AiPruneWhat>({ audit: true, recordings: true, backups: true, runs_db: true });
  let wipeText = $state("");

  let plan = $state.raw<AiPrunePlan | null>(null);
  let planning = $state(false);
  let planError = $state<string | null>(null);
  let running = $state(false);
  let runError = $state<string | null>(null);
  let outcome = $state.raw<AiPruneOutcome | null>(null);
  let planSeq = 0;

  const endedRuns = $derived(runList.filter((r) => r.ended));
  const effectiveSelection = $derived<AiPruneSelection | null>(
    mode === "wipe"
      ? { kind: "all" }
      : selectionKind === "older_than"
        ? Number.isInteger(days) && days >= 1
          ? { kind: "older_than", days }
          : null
        : selectionKind === "runs"
          ? picked.length > 0
            ? { kind: "runs", ids: picked }
            : null
          : { kind: "all" },
  );
  const request = $derived<AiPruneRequest | null>(
    effectiveSelection
      ? {
          selection: effectiveSelection,
          mode,
          what: mode === "wipe" ? { audit: true, recordings: true, backups: true, runs_db: true } : what,
          archive_dir: mode === "archive" ? archiveDir : null,
        }
      : null,
  );
  const wipeConfirmed = $derived(mode !== "wipe" || wipeText.trim().toLowerCase() === "wipe");
  const nothingChosen = $derived(mode !== "wipe" && !what.audit && !what.recordings && !what.backups && !what.runs_db);
  const nRuns = $derived(plan?.runs.length ?? 0);
  const canConfirm = $derived(
    !running &&
      !planning &&
      plan !== null &&
      planError === null &&
      !nothingChosen &&
      wipeConfirmed &&
      (mode === "wipe" || nRuns > 0),
  );
  const confirmLabel = $derived.by(() => {
    const size = plan ? ` (${formatBytes(plan.bytes)})` : "";
    const runs = `${nRuns} ${nRuns === 1 ? "run" : "runs"}`;
    if (mode === "wipe") return "Wipe all AI data";
    if (mode === "archive") return `Archive ${runs}${size}`;
    return `Delete ${runs}${size}`;
  });

  onMount(() => {
    void invoke<AiStorage>("ai_storage").then((s) => (storage = s)).catch(() => {});
    void invoke<AiReportSummary[]>("ai_reports", { limit: 1000 }).then((l) => (runList = l)).catch(() => {});
  });

  // Live preview, debounced; a late answer for an older request is dropped.
  $effect(() => {
    const req = request;
    if (outcome) return;
    const seq = ++planSeq;
    if (!req) {
      plan = null;
      planError = null;
      planning = false;
      return;
    }
    planning = true;
    const timer = setTimeout(async () => {
      try {
        const p = await invoke<AiPrunePlan>("ai_prune_plan", { req });
        if (seq !== planSeq) return;
        plan = p;
        planError = null;
      } catch (err) {
        if (seq !== planSeq) return;
        plan = null;
        planError = String(err);
      } finally {
        if (seq === planSeq) planning = false;
      }
    }, 250);
    return () => clearTimeout(timer);
  });

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let v = n / 1024;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${v >= 10 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
  }

  function togglePicked(id: string, on: boolean) {
    picked = on ? [...picked, id] : picked.filter((p) => p !== id);
  }

  /** The prune allowed `<archive>/manifest.json`; revealing it opens the archive folder. */
  async function revealArchive(dir: string) {
    try {
      await invoke("ai_reveal_path", { path: `${dir.replace(/[\\/]+$/, "")}/manifest.json` });
    } catch (err) {
      notify(`Could not show the archive: ${err}`, "error");
    }
  }

  async function chooseFolder() {
    try {
      const dir = await invoke<string | null>("ai_pick_archive_dir");
      if (dir) archiveDir = dir;
    } catch (err) {
      runError = `Could not open the folder picker: ${err}`;
    }
  }

  async function confirm() {
    if (!request || !canConfirm) return;
    running = true;
    runError = null;
    try {
      const result = await invoke<AiPruneOutcome>("ai_prune", { req: request });
      outcome = result;
      onDone(result);
    } catch (err) {
      runError = String(err);
    } finally {
      running = false;
    }
  }

  const title = "Clean up AI data";
</script>

<ModalShell open={true} {title} onClose={onClose} width="lg">
  <div class="modal-header">
    <h2 class="modal-title">{title}</h2>
    <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
      <X size={16} />
    </button>
  </div>

  {#if outcome}
    <div class="modal-body flex flex-col gap-3">
      <div class="alert {outcome.verify_ok ? 'alert-success' : 'alert-error'}" role="status">
        <div class="alert-title">
          {outcome.plan.mode === "wipe" ? "All AI data wiped" : `${outcome.plan.runs.length} ${outcome.plan.runs.length === 1 ? "run" : "runs"} ${outcome.plan.mode === "archive" ? "archived" : "deleted"}`}
        </div>
        <p class="m-0 text-xs">
          Freed about {formatBytes(outcome.plan.bytes)}. The audit log went from
          {formatBytes(outcome.plan.log_bytes_before)} to {formatBytes(outcome.log_bytes_after)}.
        </p>
        <p class="m-0 text-xs" data-testid="verify-status">
          {outcome.verify_ok ? "The audit log still verifies." : "The audit log did NOT verify after the clean-up. Keep a copy of the ai/audit folder and check `bssh ai log` before pruning again."}
        </p>
      </div>
      {#if outcome.archive_dir}
        <div>
          <div class="type-meta">Archive</div>
          <p class="mono m-0 break-all text-sm text-primary" data-testid="archive-path">{outcome.archive_dir}</p>
          <button type="button" class="btn btn-ghost mt-1" onclick={() => revealArchive(outcome?.archive_dir ?? "")}>
            <FolderOpen size={14} />
            Show in folder
          </button>
        </div>
      {/if}
    </div>
    <div class="modal-footer">
      <button type="button" class="btn btn-primary" onclick={onClose} data-autofocus>Done</button>
    </div>
  {:else}
    <div class="modal-body flex max-h-[70vh] flex-col gap-4 overflow-y-auto">
      <dl class="m-0 grid grid-cols-2 gap-x-6 gap-y-2 text-sm sm:grid-cols-3" aria-label="Storage used">
        <div><dt class="type-meta">Audit log</dt><dd class="m-0 tabular-nums" data-testid="st-log">{storage ? formatBytes(storage.log_bytes) : "…"}</dd></div>
        <div><dt class="type-meta">Recordings</dt><dd class="m-0 tabular-nums">{storage ? formatBytes(storage.recordings_bytes) : "…"}</dd></div>
        <div><dt class="type-meta">Change backups</dt><dd class="m-0 tabular-nums">{storage ? formatBytes(storage.backups_bytes) : "…"}</dd></div>
        <div><dt class="type-meta">Runs database</dt><dd class="m-0 tabular-nums">{storage ? formatBytes(storage.runs_db_bytes) : "…"}</dd></div>
        <div><dt class="type-meta">Archives</dt><dd class="m-0 tabular-nums">{storage ? formatBytes(storage.archives_bytes) : "…"}</dd></div>
        <div>
          <dt class="type-meta">Runs</dt>
          <dd class="m-0 tabular-nums">
            {storage ? storage.runs : "…"}{#if storage?.oldest}<span class="text-muted"> since {formatDateTime(storage.oldest, timezone)}</span>{/if}
          </dd>
        </div>
      </dl>

      <fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
        <legend class="field-label mb-1">What to clean</legend>
        <div class="flex flex-col gap-2" role="radiogroup" aria-label="Runs to clean">
          <label class="flex items-center gap-2 text-sm">
            <input type="radio" name="sel" value="older_than" bind:group={selectionKind} disabled={mode === "wipe"} />
            Runs older than
            <input
              type="number"
              min="1"
              class="input w-20"
              aria-label="Days"
              bind:value={days}
              disabled={mode === "wipe" || selectionKind !== "older_than"}
              onfocus={() => (selectionKind = "older_than")}
            />
            days
            {#each PRESETS as p (p)}
              <button
                type="button"
                class="btn btn-sm {days === p && selectionKind === 'older_than' ? 'btn-secondary' : 'btn-ghost'}"
                aria-pressed={days === p && selectionKind === "older_than"}
                disabled={mode === "wipe"}
                onclick={() => {
                  selectionKind = "older_than";
                  days = p;
                }}
              >
                {p}d
              </button>
            {/each}
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input type="radio" name="sel" value="runs" bind:group={selectionKind} disabled={mode === "wipe"} />
            Selected runs
            <span class="text-muted">({picked.length})</span>
          </label>
          {#if selectionKind === "runs" && mode !== "wipe"}
            <ul class="m-0 flex max-h-40 list-none flex-col gap-1 overflow-y-auto p-0 pl-6" aria-label="Runs">
              {#each runList as r (r.run_id)}
                <li>
                  <label class="flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      class="checkbox"
                      checked={picked.includes(r.run_id)}
                      disabled={!r.ended}
                      onchange={(e) => togglePicked(r.run_id, e.currentTarget.checked)}
                    />
                    <span class="truncate">{r.client || "Unknown agent"}</span>
                    <span class="mono text-xs text-muted">{shortId(r.run_id)}</span>
                    {#if r.started}<span class="text-xs text-muted">{formatDateTime(r.started, timezone)}</span>{/if}
                    {#if !r.ended}<span class="badge badge-neutral">Not ended</span>{/if}
                  </label>
                </li>
              {/each}
            </ul>
          {/if}
          <label class="flex items-center gap-2 text-sm">
            <input type="radio" name="sel" value="all" bind:group={selectionKind} disabled={mode === "wipe"} />
            All ended runs
            {#if endedRuns.length}<span class="text-muted">({endedRuns.length})</span>{/if}
          </label>
        </div>
      </fieldset>

      <fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
        <legend class="field-label mb-1">Action</legend>
        <div class="flex flex-col gap-2" role="radiogroup" aria-label="Action">
          <label class="flex items-start gap-2 text-sm">
            <input type="radio" name="mode" value="delete" bind:group={mode} class="mt-1" />
            <span>
              <span class="font-medium text-primary">Delete</span>
              <span class="block text-xs text-muted">Remove the data. Log entries become signed stubs, so the chain still verifies.</span>
            </span>
          </label>
          <label class="flex items-start gap-2 text-sm">
            <input type="radio" name="mode" value="archive" bind:group={mode} class="mt-1" />
            <span>
              <span class="font-medium text-primary">Archive</span>
              <span class="block text-xs text-muted">Copy everything to a folder first, then remove it here.</span>
            </span>
          </label>
          {#if mode === "archive"}
            <div class="flex items-center gap-2 pl-6">
              <span class="mono min-w-0 flex-1 truncate text-xs text-secondary" title={archiveDir ?? ""} data-testid="archive-dir">
                {archiveDir ?? "Default: the archive folder inside the AI data directory"}
              </span>
              <button type="button" class="btn btn-secondary btn-sm" onclick={chooseFolder}>
                <FolderOpen size={14} />
                Choose folder…
              </button>
              {#if archiveDir}
                <button type="button" class="btn btn-ghost btn-sm" onclick={() => (archiveDir = null)}>Use default</button>
              {/if}
            </div>
          {/if}
          <label class="flex items-start gap-2 text-sm">
            <input type="radio" name="mode" value="wipe" bind:group={mode} class="mt-1" />
            <span>
              <span class="font-medium text-error">Wipe everything</span>
              <span class="block text-xs text-muted">Reset the audit chain and remove every AI recording, backup and run record. Applies to all runs.</span>
            </span>
          </label>
        </div>
      </fieldset>

      {#if mode !== "wipe"}
        <fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
          <legend class="field-label mb-1">Include</legend>
          <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" bind:checked={what.audit} /> Audit log entries</label>
          <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" bind:checked={what.recordings} /> Recordings</label>
          <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" bind:checked={what.backups} /> Change backups</label>
          {#if what.backups}
            <p class="field-hint m-0 pl-6 text-warning">Undo points are lost: changes from these runs can no longer be undone.</p>
          {/if}
          <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" bind:checked={what.runs_db} /> Runs database (runs, approvals, grants)</label>
        </fieldset>
      {:else}
        <div class="alert alert-error">
          <div class="alert-title">This cannot be undone</div>
          <p class="m-0 text-xs">
            Every AI run record, recording and undo point is removed and the audit chain starts over. Active runs block a wipe.
            Type <span class="mono">wipe</span> to confirm.
          </p>
          <input
            type="text"
            class="input mt-2"
            aria-label="Type wipe to confirm"
            autocomplete="off"
            spellcheck="false"
            bind:value={wipeText}
          />
        </div>
      {/if}

      <section class="panel p-3 text-sm" aria-label="Preview" aria-live="polite" aria-busy={planning}>
        {#if !request}
          <p class="m-0 text-muted">Choose which runs to clean.</p>
        {:else if planError}
          <p class="m-0 text-error" role="alert">{planError}</p>
        {:else if !plan}
          <p class="m-0 text-muted">Working out what would be removed…</p>
        {:else}
          <p class="m-0 text-primary" data-testid="plan-summary">
            {plan.runs.length} {plan.runs.length === 1 ? "run" : "runs"}, {plan.entries} log {plan.entries === 1 ? "entry" : "entries"},
            {plan.recordings} {plan.recordings === 1 ? "recording" : "recordings"}, about {formatBytes(plan.bytes)}
          </p>
          {#if plan.skipped_active.length > 0}
            <p class="m-0 mt-1 text-xs text-warning" data-testid="skipped-active">
              {plan.skipped_active.length} active {plan.skipped_active.length === 1 ? "run is" : "runs are"} skipped: {plan.skipped_active.map(shortId).join(", ")}
            </p>
          {/if}
          {#if plan.runs.length > 0}
            <ul class="m-0 mt-2 flex max-h-32 list-none flex-col gap-1 overflow-y-auto p-0 text-xs text-secondary" aria-label="Runs to be cleaned">
              {#each plan.runs as r (r.run_id)}
                <li class="flex gap-2">
                  <span class="truncate">{r.client || "Unknown agent"}</span>
                  <span class="mono text-muted">{shortId(r.run_id)}</span>
                  {#if r.ended}<span class="text-muted">ended {formatDateTime(r.ended, timezone)}</span>{/if}
                </li>
              {/each}
            </ul>
          {:else if mode !== "wipe"}
            <p class="m-0 mt-1 text-xs text-muted">No run matches.</p>
          {/if}
        {/if}
      </section>

      {#if runError}
        <div class="alert alert-error" role="alert">
          <div class="alert-title">The clean-up failed</div>
          <p class="m-0 text-xs">{runError}</p>
        </div>
      {/if}
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-ghost mr-auto" onclick={onClose}>Cancel</button>
      <button
        type="button"
        class="btn {mode === 'archive' ? 'btn-primary' : 'btn-danger'}"
        disabled={!canConfirm}
        title={nothingChosen ? "Pick at least one thing to include" : !wipeConfirmed ? "Type wipe to confirm" : undefined}
        onclick={confirm}
      >
        {#if running}<span class="spinner size-3" aria-hidden="true"></span>{/if}
        {confirmLabel}
      </button>
    </div>
  {/if}
</ModalShell>
