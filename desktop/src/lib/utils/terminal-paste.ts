import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { mount, unmount } from "svelte";
import PasteConfirm from "$lib/components/terminal/PasteConfirm.svelte";
import { readClipboardText } from "$lib/utils/terminal-xterm";

const IS_LINUX =
  typeof navigator !== "undefined" &&
  /Linux/i.test(navigator.platform || navigator.userAgent || "");

let lastWindowSelection = "";
let activeConfirmDialog = false;

function trimOneTrailingNewline(text: string): string {
  if (text.endsWith("\r\n")) {
    return text.slice(0, -2);
  }
  if (text.endsWith("\n") || text.endsWith("\r")) {
    return text.slice(0, -1);
  }
  return text;
}

/** Paste text into the terminal; asks confirmation first when text contains a newline/CR
 *  (after trimming ONE trailing newline). Resolves true if pasted. */
export async function pasteIntoTerminal(term: Terminal, text: string): Promise<boolean> {
  const trimmed = trimOneTrailingNewline(text);
  const isMultiLine = /\r|\n/.test(trimmed);

  if (!isMultiLine) {
    try {
      term.paste(text);
      try {
        term.focus();
      } catch {
        // Terminal might have been disposed
      }
      return true;
    } catch {
      return false;
    }
  }

  // Multi-line paste requires confirmation.
  // Only one dialog at a time: reject concurrent requests.
  if (activeConfirmDialog) {
    return false;
  }
  activeConfirmDialog = true;

  // Not Promise.withResolvers: older WebKit webviews (macOS < 14.4) lack it,
  // and throwing here would leave activeConfirmDialog stuck on.
  let resolve!: (pasted: boolean) => void;
  const promise = new Promise<boolean>((r) => {
    resolve = r;
  });
  let settled = false;
  let component: Record<string, any> | null = null;

  const closeDialog = (confirmed: boolean) => {
    if (settled) return;
    settled = true;
    activeConfirmDialog = false;

    if (component) {
      try {
        void unmount(component);
      } catch {
        // Ignore unmount error if already unmounted
      }
      component = null;
    }

    try {
      term.focus();
    } catch {
      // Terminal might have been disposed
    }

    if (confirmed) {
      try {
        term.paste(text);
        resolve(true);
      } catch {
        resolve(false);
      }
    } else {
      resolve(false);
    }
  };

  try {
    component = mount(PasteConfirm, {
      target: document.body,
      props: {
        text,
        onConfirm: () => closeDialog(true),
        onCancel: () => closeDialog(false),
      },
    });
  } catch {
    activeConfirmDialog = false;
    try {
      term.focus();
    } catch {
      // Terminal might have been disposed
    }
    resolve(false);
  }

  return promise;
}

/** Capture-phase `paste` listener on the container: preventDefault + stopPropagation, read
 *  clipboardData text, route through pasteIntoTerminal. Returns cleanup. */
export function attachPasteGuard(container: HTMLElement, term: Terminal): () => void {
  const onPaste = (e: ClipboardEvent) => {
    e.preventDefault();
    e.stopPropagation();

    const text =
      e.clipboardData?.getData("text/plain") ||
      e.clipboardData?.getData("text") ||
      "";

    if (text) {
      void pasteIntoTerminal(term, text);
    }
  };

  container.addEventListener("paste", onPaste, true);
  return () => {
    container.removeEventListener("paste", onPaste, true);
  };
}

/** Middle-click (auxclick button 1) paste. Linux: invoke("read_primary_selection"); if null,
 *  fall back to the most recent selection made in ANY xterm of this window (track via
 *  term.onSelectionChange; register each term). Other OS: the system clipboard.
 *  Routes through pasteIntoTerminal. Returns cleanup (also unregisters the selection tracker). */
export function attachMiddleClickPaste(container: HTMLElement, term: Terminal): () => void {
  // Track selections made in this terminal for window-wide fallback
  const selectionDisposable = term.onSelectionChange(() => {
    try {
      const sel = term.getSelection();
      if (sel && sel.length > 0) {
        lastWindowSelection = sel;
      }
    } catch {
      // Terminal may be disposed
    }
  });

  const onAuxClick = async (e: MouseEvent) => {
    if (e.button !== 1) return;
    e.preventDefault();
    e.stopPropagation();

    if (IS_LINUX) {
      let primaryText: string | null = null;
      try {
        primaryText = await invoke<string | null>("read_primary_selection");
      } catch {
        primaryText = null;
      }

      if (primaryText) {
        void pasteIntoTerminal(term, primaryText);
        return;
      }

      // Fallback to the most recent selection in any registered xterm of this window
      if (lastWindowSelection) {
        void pasteIntoTerminal(term, lastWindowSelection);
      }
    } else {
      const clipText = await readClipboardText();
      if (clipText) {
        void pasteIntoTerminal(term, clipText);
      }
    }
  };

  container.addEventListener("auxclick", onAuxClick);

  return () => {
    container.removeEventListener("auxclick", onAuxClick);
    try {
      selectionDisposable.dispose();
    } catch {
      // Ignore if already disposed
    }
  };
}
