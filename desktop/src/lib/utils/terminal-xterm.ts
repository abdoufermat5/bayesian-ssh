import { invoke } from "@tauri-apps/api/core";
import type { Terminal } from "@xterm/xterm";

import { attachOrphanCompositionEndGuard } from "$lib/utils/terminal-composition-guard";

function writePty(sessionId: string, data: string): void {
  invoke("write_pty", { sessionId, data }).catch((err) => {
    console.error("write_pty failed:", err);
  });
}

/** Wire xterm keyboard output to the backend PTY session. */
export function attachXtermIo(sessionId: string, term: Terminal): void {
  term.onData((data) => writePty(sessionId, data));
  term.onBinary((data) => writePty(sessionId, data));
}

/** Copy/paste shortcuts and Linux WebKitGTK IME guards. */
export function attachXtermKeyHandler(term: Terminal): void {
  term.attachCustomKeyEventHandler((event) => {
    if (event.isComposing) {
      return false;
    }

    const key = event.key.toLowerCase();

    if (
      (event.ctrlKey && key === "c" && term.hasSelection()) ||
      (event.ctrlKey && event.shiftKey && key === "c")
    ) {
      if (event.type === "keydown") {
        const selection = term.getSelection();
        if (selection) {
          navigator.clipboard.writeText(selection).then(() => {
            term.clearSelection();
          }).catch((err) => {
            console.error("Failed to copy to clipboard", err);
          });
        }
      }
      return false;
    }

    if (
      (event.ctrlKey && key === "v") ||
      (event.ctrlKey && event.shiftKey && key === "v") ||
      (event.shiftKey && event.key === "insert")
    ) {
      if (event.type === "keydown") {
        navigator.clipboard.readText().then((text) => {
          if (text) {
            term.paste(text);
          }
        }).catch((err) => {
          console.error("Failed to read from clipboard", err);
        });
      }
      return false;
    }

    return true;
  });
}

/**
 * Fix WebKitGTK+IBUS orphan composition events that break xterm input on Linux.
 * Returns a cleanup function — call before term.dispose().
 */
export function attachXtermLinuxInputFix(
  container: HTMLElement,
  term: Terminal,
): () => void {
  return attachOrphanCompositionEndGuard(container, (data) => {
    term.input(data, true);
  });
}
