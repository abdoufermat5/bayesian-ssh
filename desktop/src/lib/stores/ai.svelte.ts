import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AiApprovalRequest, AiRun, AiStoredEvent } from "$lib/types";
import { notify } from "$lib/stores/notifications.svelte";

/**
 * Live state of bssh AI runs. The Rust side (`commands/ai.rs`) keeps one
 * socket per `bssh ai mcp` process and pushes three events:
 *  - `ai://runs`: every run, after any change (replaces the list);
 *  - `ai://event`: one timeline entry of a run;
 *  - `ai://request`: a new approval request (opens the approval dialog).
 */

/** A pending request with the run it belongs to. */
export interface PendingApproval {
  run: AiRun;
  request: AiApprovalRequest;
}

let runs = $state.raw<AiRun[]>([]);
let events = $state.raw<Record<string, AiStoredEvent[]>>({});
let dialogOpen = $state(false);
let dialogRequestId = $state<string | null>(null);
/** Requests answered here and waiting for the run to confirm: hidden from the dialog. */
let answered = $state.raw<Set<string>>(new Set());

let unlisten: UnlistenFn[] = [];
let starting: Promise<void> | null = null;

export function getAiRuns(): AiRun[] {
  return runs;
}

export function getAiRun(runId: string | null): AiRun | null {
  return runId ? (runs.find((r) => r.run_id === runId) ?? null) : null;
}

export function getAiEvents(runId: string): AiStoredEvent[] {
  return events[runId] ?? [];
}

/** Open requests across all runs, oldest first (the one nearest to expiring leads). */
export function pendingApprovals(): PendingApproval[] {
  return runs
    .flatMap((run) => run.pending.map((request) => ({ run, request })))
    .filter((p) => !answered.has(p.request.request_id))
    .sort((a, b) => Date.parse(a.request.created_at) - Date.parse(b.request.created_at));
}

export function aiPendingCount(): number {
  return pendingApprovals().length;
}

export function approvalDialogOpen(): boolean {
  return dialogOpen && pendingApprovals().length > 0;
}

/** The request the dialog shows: the chosen one, else the first waiting. */
export function currentApproval(): PendingApproval | null {
  const list = pendingApprovals();
  return list.find((p) => p.request.request_id === dialogRequestId) ?? list[0] ?? null;
}

export function openApproval(requestId?: string) {
  dialogRequestId = requestId ?? null;
  dialogOpen = true;
}

export function closeApproval() {
  dialogOpen = false;
}

/** Step through the waiting requests by `delta` (wraps). */
export function stepApproval(delta: number) {
  const list = pendingApprovals();
  if (list.length === 0) return;
  const at = Math.max(
    0,
    list.findIndex((p) => p.request.request_id === currentApproval()?.request.request_id),
  );
  dialogRequestId = list[(at + delta + list.length) % list.length].request.request_id;
}

/** Merge by `seq`: the live stream and the initial fetch can overlap. */
function mergeEvents(runId: string, incoming: AiStoredEvent[]) {
  if (incoming.length === 0) return;
  const known = events[runId] ?? [];
  const seen = new Set(known.map((e) => e.seq));
  const fresh = incoming.filter((e) => !seen.has(e.seq));
  if (fresh.length === 0) return;
  events = { ...events, [runId]: [...known, ...fresh].sort((a, b) => a.seq - b.seq) };
}

async function loadEvents(runId: string) {
  try {
    mergeEvents(runId, await invoke<AiStoredEvent[]>("ai_run_events", { runId }));
  } catch (err) {
    console.error("Failed to load AI run events", err);
  }
}

function setRuns(next: AiRun[]) {
  const known = new Set(Object.keys(events));
  runs = next;
  for (const run of next) {
    if (!known.has(run.run_id)) void loadEvents(run.run_id);
  }
  // Answered requests that are gone from every run are settled.
  const open = new Set(next.flatMap((r) => r.pending.map((p) => p.request_id)));
  const still = new Set([...answered].filter((id) => open.has(id)));
  if (still.size !== answered.size) answered = still;
}

/** Subscribe to the backend and load the current runs. Safe to call twice. */
export function startAi(): Promise<void> {
  if (starting) return starting;
  starting = (async () => {
    unlisten = await Promise.all([
      listen<AiRun[]>("ai://runs", (e) => setRuns(e.payload)),
      listen<AiStoredEvent & { run_id: string }>("ai://event", (e) => {
        const { run_id, ...stored } = e.payload;
        mergeEvents(run_id, [stored]);
      }),
      listen<{ run_id: string; request: AiApprovalRequest }>("ai://request", (e) => {
        openApproval(e.payload.request.request_id);
      }),
      listen<{ run_id: string; message: string }>("ai://error", (e) => {
        // The run refused or failed something we sent: show those requests again.
        answered = new Set();
        notify(e.payload.message, "error");
      }),
    ]);
    try {
      setRuns(await invoke<AiRun[]>("ai_list_runs"));
      if (pendingApprovals().length > 0) openApproval();
    } catch (err) {
      console.error("Failed to list AI runs", err);
    }
  })();
  return starting;
}

export function stopAiListeners() {
  for (const off of unlisten) off();
  unlisten = [];
  starting = null;
}

/** Approve or deny one request. Throws the backend's message on failure. */
export async function decideApproval(
  runId: string,
  requestId: string,
  approve: boolean,
  reason?: string,
  grantMinutes?: number,
) {
  await invoke("ai_decide", {
    runId,
    requestId,
    approve,
    reason: reason?.trim() ? reason.trim() : null,
    grantMinutes: grantMinutes ?? null,
  });
  answered = new Set([...answered, requestId]);
}

export async function stopAiRun(runId: string) {
  await invoke("ai_stop", { runId });
}

export async function revokeAiGrant(runId: string, grantId: string) {
  await invoke("ai_revoke_grant", { runId, grantId });
}
