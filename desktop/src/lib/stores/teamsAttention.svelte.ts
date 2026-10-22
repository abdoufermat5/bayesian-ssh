import { invoke } from "@tauri-apps/api/core";
import type { TeamAttention, TeamsStatus } from "$lib/types";
import {
  TEAM_ATTENTION,
  raiseAttention,
  resolveAttention,
  resolveAttentionPrefix,
  resolveAttentionSuffix,
} from "$lib/stores/attention.svelte";

/** App-level poller: surfaces Teams conditions that need a human, on any view. */
const ATTENTION_INTERVAL_MS = 60_000;
/** Local-only check (no network): notices unlock/lock soon after they happen. */
const STATUS_INTERVAL_MS = 10_000;

/** Views dispatch this on `window` after unlock, sync, grant or remove to refresh right away. */
export const TEAMS_CHANGED_EVENT = "bssh:teams-changed";

let statusTimer: number | undefined;
let lastAttentionAt = 0;
let wasUnlocked = false;
let running = false;
let busy = false;

function apply(attention: TeamAttention[], names: Map<string, string>) {
  for (const a of attention) {
    const name = names.get(a.team_id) ?? a.team_id;

    if (a.role === "admin" && a.pending_devices > 0) {
      const n = a.pending_devices;
      raiseAttention({
        id: TEAM_ATTENTION.pending(a.team_id),
        severity: "warning",
        title: `${n} ${n === 1 ? "device is" : "devices are"} waiting for the ${name} key`,
        body: "Check each device's fingerprint with its owner, out of band, before granting the key.",
        action: { label: "Review devices", tab: "teams", teamId: a.team_id },
      });
    } else {
      resolveAttention(TEAM_ATTENTION.pending(a.team_id));
    }

    if (a.waiting_for_key && !a.removed) {
      raiseAttention({
        id: TEAM_ATTENTION.waitingKey(a.team_id),
        severity: "info",
        title: `Waiting for an admin to grant this device the ${name} key`,
        body: "Give your admin this device's fingerprint (shown in Teams) through a channel you trust.",
        action: { label: "Show fingerprint", tab: "teams", teamId: a.team_id },
        dismissible: false,
      });
    } else {
      resolveAttention(TEAM_ATTENTION.waitingKey(a.team_id));
    }

    if (a.removed) {
      raiseAttention({
        id: TEAM_ATTENTION.removed(a.team_id),
        severity: "error",
        title: `You were removed from ${name}`,
        body: "This account can no longer sync this team. Local copies stay until you remove them.",
        action: { label: "Open Teams", tab: "teams", teamId: a.team_id },
      });
    } else {
      resolveAttention(TEAM_ATTENTION.removed(a.team_id));
    }
  }
}

const raisedTeams = new Set<string>();

/** One poll round. Never throws; failures go to the console. */
export async function refreshTeamsAttention(force = false): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    const status = await invoke<TeamsStatus>("teams_status");
    if (!status.unlocked) {
      if (wasUnlocked || raisedTeams.size > 0) {
        resolveAttentionPrefix("teams:pending:");
        resolveAttentionPrefix("teams:waiting-key:");
        resolveAttentionPrefix("teams:removed:");
        raisedTeams.clear();
      }
      wasUnlocked = false;
      return;
    }
    const justUnlocked = !wasUnlocked;
    wasUnlocked = true;
    const due = Date.now() - lastAttentionAt >= ATTENTION_INTERVAL_MS;
    if (!(force || justUnlocked || due)) return;

    lastAttentionAt = Date.now();
    const attention = await invoke<TeamAttention[]>("teams_attention");
    const names = new Map(status.teams.map((t) => [t.team_id, t.name]));
    apply(attention, names);
    const ids = new Set(status.teams.map((t) => t.team_id));
    for (const a of attention) raisedTeams.add(a.team_id);
    // A team that disappeared locally can't have open conditions.
    for (const t of [...raisedTeams]) {
      if (ids.has(t)) continue;
      resolveAttentionSuffix(`:${t}`);
      raisedTeams.delete(t);
    }
  } catch (e) {
    console.error("teams attention poll failed", e);
  } finally {
    busy = false;
  }
}

function onTeamsChanged() {
  void refreshTeamsAttention(true);
}

export function startTeamsAttentionPoller() {
  if (running) return;
  running = true;
  void refreshTeamsAttention();
  statusTimer = window.setInterval(() => void refreshTeamsAttention(), STATUS_INTERVAL_MS);
  window.addEventListener(TEAMS_CHANGED_EVENT, onTeamsChanged);
}

export function stopTeamsAttentionPoller() {
  running = false;
  window.clearInterval(statusTimer);
  window.removeEventListener(TEAMS_CHANGED_EVENT, onTeamsChanged);
}
