import { popOutTab } from "$lib/stores/terminal.svelte";

const DRAG_THRESHOLD_PX = 28;

export function tabPopOutDrag(node: HTMLElement, tabId: string) {
  let startX = 0;
  let startY = 0;
  let armed = false;
  let popped = false;

  function onPointerDown(event: PointerEvent) {
    if ((event.target as HTMLElement).closest("button")) return;

    armed = true;
    popped = false;
    startX = event.clientX;
    startY = event.clientY;
    node.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent) {
    if (!armed || popped) return;

    const dx = event.clientX - startX;
    const dy = event.clientY - startY;
    if (Math.abs(dx) >= DRAG_THRESHOLD_PX || Math.abs(dy) >= DRAG_THRESHOLD_PX) {
      node.classList.add("tab-dragging-out");
    } else {
      node.classList.remove("tab-dragging-out");
    }
  }

  async function onPointerUp(event: PointerEvent) {
    if (!armed) return;

    const dx = event.clientX - startX;
    const dy = event.clientY - startY;
    const shouldPopOut =
      !popped && (Math.abs(dx) >= DRAG_THRESHOLD_PX || Math.abs(dy) >= DRAG_THRESHOLD_PX);

    armed = false;
    node.classList.remove("tab-dragging-out");

    if (node.hasPointerCapture(event.pointerId)) {
      node.releasePointerCapture(event.pointerId);
    }

    if (shouldPopOut) {
      popped = true;
      await popOutTab(tabId);
    }
  }

  node.addEventListener("pointerdown", onPointerDown);
  node.addEventListener("pointermove", onPointerMove);
  node.addEventListener("pointerup", onPointerUp);
  node.addEventListener("pointercancel", onPointerUp);

  return {
    update(nextTabId: string) {
      tabId = nextTabId;
      popped = false;
      armed = false;
      node.classList.remove("tab-dragging-out");
    },
    destroy() {
      node.removeEventListener("pointerdown", onPointerDown);
      node.removeEventListener("pointermove", onPointerMove);
      node.removeEventListener("pointerup", onPointerUp);
      node.removeEventListener("pointercancel", onPointerUp);
      node.classList.remove("tab-dragging-out");
    },
  };
}
