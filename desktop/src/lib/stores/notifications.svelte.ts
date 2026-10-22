import type { NotificationType } from "$lib/types";

export interface ToastItem {
  id: number;
  text: string;
  type: NotificationType;
}

/** Non-error toasts beyond this count push the oldest non-error one out. */
const MAX_VISIBLE = 3;
/** Success/info toasts time out; errors never do (WCAG 2.2.1). */
const DURATION_MS = 4000;

let toasts = $state<ToastItem[]>([]);
const timers = new Map<number, number>();
let nextId = 1;

function schedule(id: number) {
  clearTimeout(timers.get(id));
  timers.set(id, window.setTimeout(() => dismissNotification(id), DURATION_MS));
}

export function notify(message: string, notificationType: NotificationType = "info") {
  // Repeating the newest toast restarts its timer (errors have none) instead of stacking a copy.
  const last = toasts[toasts.length - 1];
  if (last && last.text === message && last.type === notificationType) {
    if (notificationType !== "error") schedule(last.id);
    return;
  }

  const id = nextId++;
  const next = [...toasts, { id, text: message, type: notificationType }];
  // Evict the oldest non-error toasts only: an error is never dropped, it
  // stays until the user closes it, so the stack may exceed MAX_VISIBLE.
  while (next.length > MAX_VISIBLE) {
    const victim = next.findIndex((t) => t.type !== "error" && t.id !== id);
    if (victim === -1) break;
    const [dropped] = next.splice(victim, 1);
    clearTimeout(timers.get(dropped.id));
    timers.delete(dropped.id);
  }
  toasts = next;
  if (notificationType !== "error") schedule(id);
}

/** Dismiss one toast by id, or every toast when no id is given. */
export function dismissNotification(id?: number) {
  if (id === undefined) {
    for (const timer of timers.values()) clearTimeout(timer);
    timers.clear();
    toasts = [];
    return;
  }
  clearTimeout(timers.get(id));
  timers.delete(id);
  toasts = toasts.filter((t) => t.id !== id);
}

export function getToasts(): ToastItem[] {
  return toasts;
}
