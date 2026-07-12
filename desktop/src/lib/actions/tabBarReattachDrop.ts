export const SESSION_DRAG_MIME = "application/x-bssh-session";

export interface SessionDragPayload {
  sessionId: string;
  kind: "detached" | "popout";
  name: string;
}

export function encodeSessionDrag(payload: SessionDragPayload): string {
  return JSON.stringify(payload);
}

export function decodeSessionDrag(raw: string): SessionDragPayload | null {
  try {
    const parsed = JSON.parse(raw) as SessionDragPayload;
    if (!parsed?.sessionId || !parsed?.kind) return null;
    return parsed;
  } catch {
    return null;
  }
}

export function tabBarReattachDrop(
  node: HTMLElement,
  onDropSession: (payload: SessionDragPayload) => void | Promise<void>,
) {
  let dragDepth = 0;

  function onDragEnter(event: DragEvent) {
    if (!event.dataTransfer?.types.includes(SESSION_DRAG_MIME)) return;
    event.preventDefault();
    dragDepth += 1;
    node.classList.add("tab-bar-drop-active");
  }

  function onDragOver(event: DragEvent) {
    if (!event.dataTransfer?.types.includes(SESSION_DRAG_MIME)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    node.classList.add("tab-bar-drop-active");
  }

  function onDragLeave() {
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) {
      node.classList.remove("tab-bar-drop-active");
    }
  }

  async function onDrop(event: DragEvent) {
    event.preventDefault();
    dragDepth = 0;
    node.classList.remove("tab-bar-drop-active");

    const raw = event.dataTransfer?.getData(SESSION_DRAG_MIME);
    if (!raw) return;

    const payload = decodeSessionDrag(raw);
    if (!payload) return;

    await onDropSession(payload);
  }

  node.addEventListener("dragenter", onDragEnter);
  node.addEventListener("dragover", onDragOver);
  node.addEventListener("dragleave", onDragLeave);
  node.addEventListener("drop", onDrop);

  return {
    destroy() {
      node.removeEventListener("dragenter", onDragEnter);
      node.removeEventListener("dragover", onDragOver);
      node.removeEventListener("dragleave", onDragLeave);
      node.removeEventListener("drop", onDrop);
      node.classList.remove("tab-bar-drop-active");
    },
  };
}
