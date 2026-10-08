import type { NotificationType } from "$lib/types";

export interface ToastItem {
  id: number;
  text: string;
  type: NotificationType;
}

/** Toasts beyond this count push the oldest one out. */
const MAX_VISIBLE = 3;
const DURATION_MS = 3000;

let toasts = $state<ToastItem[]>([]);
const timers = new Map<number, number>();
let nextId = 1;

function schedule(id: number) {
  clearTimeout(timers.get(id));
  timers.set(id, window.setTimeout(() => dismissNotification(id), DURATION_MS));
}

export function notify(message: string, notificationType: NotificationType = "info") {
  // Repeating the newest toast restarts its timer instead of stacking a copy.
  const last = toasts[toasts.length - 1];
  if (last && last.text === message && last.type === notificationType) {
    schedule(last.id);
    return;
  }

  const id = nextId++;
  const next = [...toasts, { id, text: message, type: notificationType }];
  for (const dropped of next.splice(0, Math.max(0, next.length - MAX_VISIBLE))) {
    clearTimeout(timers.get(dropped.id));
    timers.delete(dropped.id);
  }
  toasts = next;
  schedule(id);
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
