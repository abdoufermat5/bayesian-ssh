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

/** Clear every item whose id starts with `prefix`, e.g. all items for one team. */
export function resolveAttentionPrefix(prefix: string) {
  items = items.filter((i) => !i.id.startsWith(prefix));
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
