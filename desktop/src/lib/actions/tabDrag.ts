/**
 * Pointer drag for a terminal tab. The view decides what a position means
 * (reorder in the strip, split the terminal area, pop out); this action only
 * tracks the gesture: it starts past a small threshold, Esc cancels it, and
 * the click that ends a drag does not also activate the tab.
 */
export interface TabDragHandlers {
  /** Called on every move once the drag has started. */
  move(x: number, y: number): void;
  drop(x: number, y: number): void | Promise<void>;
  cancel(): void;
}

const DRAG_START_PX = 6;

export function tabDrag(node: HTMLElement, handlers: TabDragHandlers) {
  let pointerId: number | null = null;
  let startX = 0;
  let startY = 0;
  let dragging = false;

  function end() {
    if (pointerId !== null && node.hasPointerCapture(pointerId)) node.releasePointerCapture(pointerId);
    pointerId = null;
    dragging = false;
    node.classList.remove("tab-dragging");
    window.removeEventListener("keydown", onKeydown, true);
  }

  function swallowNextClick() {
    const stop = (e: Event) => {
      e.stopPropagation();
      e.preventDefault();
    };
    node.addEventListener("click", stop, { capture: true, once: true });
    // A drop outside the tab fires no click; don't eat a later real one.
    setTimeout(() => node.removeEventListener("click", stop, { capture: true }), 0);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape" || !dragging) return;
    e.preventDefault();
    e.stopPropagation();
    end();
    handlers.cancel();
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    pointerId = e.pointerId;
    startX = e.clientX;
    startY = e.clientY;
    node.setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (e.pointerId !== pointerId) return;
    if (!dragging) {
      if (Math.hypot(e.clientX - startX, e.clientY - startY) < DRAG_START_PX) return;
      dragging = true;
      node.classList.add("tab-dragging");
      window.addEventListener("keydown", onKeydown, true);
    }
    handlers.move(e.clientX, e.clientY);
  }

  function onPointerUp(e: PointerEvent) {
    if (e.pointerId !== pointerId) return;
    const wasDragging = dragging;
    end();
    if (!wasDragging) return;
    swallowNextClick();
    void handlers.drop(e.clientX, e.clientY);
  }

  // The webview took the pointer (e.g. a native drag): never act on it.
  function onPointerCancel(e: PointerEvent) {
    if (e.pointerId !== pointerId) return;
    const wasDragging = dragging;
    end();
    if (wasDragging) handlers.cancel();
  }

  node.addEventListener("pointerdown", onPointerDown);
  node.addEventListener("pointermove", onPointerMove);
  node.addEventListener("pointerup", onPointerUp);
  node.addEventListener("pointercancel", onPointerCancel);

  return {
    update(next: TabDragHandlers) {
      handlers = next;
    },
    destroy() {
      end();
      node.removeEventListener("pointerdown", onPointerDown);
      node.removeEventListener("pointermove", onPointerMove);
      node.removeEventListener("pointerup", onPointerUp);
      node.removeEventListener("pointercancel", onPointerCancel);
    },
  };
}
