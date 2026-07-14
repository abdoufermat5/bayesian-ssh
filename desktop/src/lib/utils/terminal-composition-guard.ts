/**
 * WebKitGTK (Tauri's Linux webview) routes text input through IBUS in ways that
 * break xterm.js's hidden textarea bookkeeping. Without this guard, keystrokes
 * can be duplicated, erased, or never reach onData — vim/nano appear frozen.
 *
 * Adapted from coollabsio/jean (terminal-composition-guard.ts).
 */
export function attachOrphanCompositionEndGuard(
  root: HTMLElement,
  deliverOrphanData?: (data: string) => void,
): () => void {
  let compositionTarget: EventTarget | null = null;
  let lastKeydownKeyCode: number | null = null;
  let pendingRestore: {
    target: HTMLTextAreaElement;
    value: string;
    deliver: string | null;
  } | null = null;

  const onKeyDown = (event: Event): void => {
    lastKeydownKeyCode = (event as KeyboardEvent).keyCode;
    pendingRestore = null;
  };

  const onCompositionStart = (event: Event): void => {
    compositionTarget = event.target;
  };

  const onCompositionEnd = (event: Event): void => {
    if (compositionTarget !== null && event.target === compositionTarget) {
      compositionTarget = null;
      return;
    }
    event.stopPropagation();
  };

  const onBeforeInput = (event: Event): void => {
    if (!deliverOrphanData) return;
    const { inputType, data } = event as InputEvent;
    const target = event.target;
    if (!data || !(target instanceof HTMLTextAreaElement)) return;

    if (inputType === "insertFromComposition" && target !== compositionTarget) {
      pendingRestore = { target, value: target.value, deliver: data };
    } else if (
      inputType === "insertText" &&
      compositionTarget === null &&
      lastKeydownKeyCode !== null &&
      lastKeydownKeyCode !== 229
    ) {
      pendingRestore = { target, value: target.value, deliver: null };
    }
  };

  const onInput = (event: Event): void => {
    if (!pendingRestore) return;
    const { target, value, deliver } = pendingRestore;
    pendingRestore = null;
    if (event.target !== target) return;
    target.value = value;
    if (deliver) {
      deliverOrphanData(deliver);
    }
  };

  root.addEventListener("keydown", onKeyDown, true);
  root.addEventListener("compositionstart", onCompositionStart, true);
  root.addEventListener("compositionend", onCompositionEnd, true);
  root.addEventListener("beforeinput", onBeforeInput, true);
  root.addEventListener("input", onInput, true);

  return () => {
    root.removeEventListener("keydown", onKeyDown, true);
    root.removeEventListener("compositionstart", onCompositionStart, true);
    root.removeEventListener("compositionend", onCompositionEnd, true);
    root.removeEventListener("beforeinput", onBeforeInput, true);
    root.removeEventListener("input", onInput, true);
  };
}
