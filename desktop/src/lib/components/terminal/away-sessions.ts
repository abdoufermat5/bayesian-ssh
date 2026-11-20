/**
 * Sessions running away from the tab strip (in their own window or in the
 * background): shown as chips in the strip and as a list in the empty view.
 */
import { notify } from "$lib/stores/notifications.svelte";
import {
  dockPopoutSession,
  focusPopoutSession,
  getTerminalState,
  reattachSession,
} from "$lib/stores/terminal.svelte";
import { encodeSessionDrag, SESSION_DRAG_MIME, type SessionDragPayload } from "$lib/actions/tabBarReattachDrop";

/** Reads reactive store state: call it inside `$derived` to track changes. */
export function listAwaySessions(): SessionDragPayload[] {
  const state = getTerminalState();
  return [
    ...state.popoutSessions.map((s) => ({ sessionId: s.id, kind: "popout" as const, name: s.name })),
    ...state.detachedSessions.map((s) => ({ sessionId: s.id, kind: "detached" as const, name: s.name })),
  ];
}

/** Dock a pop-out window or reattach a background session as a tab. */
export async function bringBackSession(payload: SessionDragPayload): Promise<void> {
  try {
    if (payload.kind === "popout") await dockPopoutSession(payload.sessionId);
    else await reattachSession(payload.sessionId);
  } catch (e: unknown) {
    notify(String(e), "error");
  }
}

/** Click on a chip: focus a pop-out's window, reattach a background session. */
export function activateAwaySession(payload: SessionDragPayload): void {
  if (payload.kind === "popout") void focusPopoutSession(payload.sessionId);
  else void bringBackSession(payload);
}

export function startAwayDrag(event: DragEvent, payload: SessionDragPayload): void {
  event.dataTransfer?.setData(SESSION_DRAG_MIME, encodeSessionDrag(payload));
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  (event.currentTarget as HTMLElement).classList.add("away-chip-dragging");
}

export function endAwayDrag(event: DragEvent): void {
  (event.currentTarget as HTMLElement).classList.remove("away-chip-dragging");
}
