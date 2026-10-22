import type { AppTab } from "$lib/types";

/**
 * Persistent attention items: things a user must not miss, unlike toasts.
 * Security-relevant events (an audit log that failed verification, entries
 * rejected by integrity checks, a broken audit chain, a blocked key rotation,
 * devices waiting for a key, this device waiting for its key) go here, never
 * only into a toast. Items stay until the condition is resolved in code
 * (`resolveAttention`) or, for informational ones, dismissed by the user.
 */
export type AttentionSeverity = "error" | "warning" | "info";

export interface AttentionAction {
  label: string;
  /** View to open. */
  tab: AppTab;
  /** Optional team to focus when `tab` is "teams". */
  teamId?: string;
}

export interface AttentionItem {
  /** Stable key, so raising the same condition twice updates one item, e.g. `audit-failed:<team_id>`. */
  id: string;
  severity: AttentionSeverity;
  title: string;
  body?: string;
  action?: AttentionAction;
  /** Errors are never dismissible: they clear when the condition is resolved. */
  dismissible: boolean;
  raisedAt: number;
}

let items = $state<AttentionItem[]>([]);

/** Add or update an item (keyed by `id`). Errors default to non-dismissible. */
export function raiseAttention(
  item: Omit<AttentionItem, "raisedAt" | "dismissible"> & { dismissible?: boolean },
) {
  const next: AttentionItem = {
    ...item,
    dismissible: item.dismissible ?? item.severity !== "error",
    raisedAt: Date.now(),
  };
  const index = items.findIndex((i) => i.id === item.id);
  items = index === -1 ? [...items, next] : items.map((i, n) => (n === index ? next : i));
}

/** Clear an item because its condition no longer holds. */
export function resolveAttention(id: string) {
  items = items.filter((i) => i.id !== id);
}

/** Clear every item whose id starts with `prefix`, e.g. `teams:`. */
export function resolveAttentionPrefix(prefix: string) {
  items = items.filter((i) => !i.id.startsWith(prefix));
}

/** Clear every item whose id ends with `suffix`, e.g. `:<team_id>` for one team. */
export function resolveAttentionSuffix(suffix: string) {
  items = items.filter((i) => !i.id.endsWith(suffix));
}

/** User dismissal; ignored for non-dismissible items. */
export function dismissAttention(id: string) {
  items = items.filter((i) => i.id !== id || !i.dismissible);
}

/** Most severe first, then newest. */
export function getAttention(): AttentionItem[] {
  const rank = { error: 0, warning: 1, info: 2 } as const;
  return [...items].sort((a, b) => rank[a.severity] - rank[b.severity] || b.raisedAt - a.raisedAt);
}

/** Count for a sidebar badge on `tab` (items whose action targets it). */
export function attentionCount(tab: AppTab): number {
  return items.filter((i) => i.action?.tab === tab).length;
}

/**
 * Item ids for Teams conditions, so every raiser (the app-level poller and
 * the Teams view) updates the same item instead of stacking duplicates.
 */
export const TEAM_ATTENTION = {
  pending: (teamId: string) => `teams:pending:${teamId}`,
  waitingKey: (teamId: string) => `teams:waiting-key:${teamId}`,
  removed: (teamId: string) => `teams:removed:${teamId}`,
  auditFailed: (teamId: string) => `teams:audit-failed:${teamId}`,
  integrity: (teamId: string) => `teams:integrity:${teamId}`,
  chainBroken: (teamId: string) => `teams:chain-broken:${teamId}`,
  rotationBlocked: (teamId: string) => `teams:rotation-blocked:${teamId}`,
} as const;

let focusTeamId = $state<string | null>(null);
let focusTeamTab: "hosts" | "members" | "audit" | null = null;

/**
 * Ask the Teams view to open a team (e.g. from an attention action). Without
 * `tab`, the view picks the tab from the team's attention items.
 */
export function requestTeamFocus(teamId: string, tab?: "hosts" | "members" | "audit") {
  focusTeamTab = tab ?? null;
  focusTeamId = teamId;
}

/** Read and clear the pending focus request; the Teams view calls this. */
export function takeTeamFocus(): string | null {
  const id = focusTeamId;
  focusTeamId = null;
  return id;
}

/** The tab asked for with the last focus request, if any; cleared on read. */
export function takeTeamFocusTab(): "hosts" | "members" | "audit" | null {
  const tab = focusTeamTab;
  focusTeamTab = null;
  return tab;
}

/** Reactive peek, for an effect that reacts to new focus requests. */
export function pendingTeamFocus(): string | null {
  return focusTeamId;
}
