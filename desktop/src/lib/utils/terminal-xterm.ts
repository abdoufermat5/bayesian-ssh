import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Terminal } from "@xterm/xterm";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { WebglAddon } from "@xterm/addon-webgl";
import { CanvasAddon } from "@xterm/addon-canvas";
import { ImageAddon } from "@xterm/addon-image";
import { Unicode11Addon } from "@xterm/addon-unicode11";

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

export interface LoadedAddons {
  searchAddon: SearchAddon;
  webLinksAddon: WebLinksAddon;
  clipboardAddon: ClipboardAddon;
  unicode11Addon: Unicode11Addon;
  imageAddon: ImageAddon;
  webglAddon?: WebglAddon;
  canvasAddon?: CanvasAddon;
}

/** Instantiates and loads web-links, search, clipboard, unicode11, image, and GPU acceleration addons onto the terminal. */
export function attachXtermAddons(term: Terminal): LoadedAddons {
  const webLinksAddon = new WebLinksAddon((event, uri) => {
    openUrl(uri).catch(() => {
      window.open(uri, "_blank");
    });
  });

  const searchAddon = new SearchAddon();
  const clipboardAddon = new ClipboardAddon();
  const unicode11Addon = new Unicode11Addon();
  const imageAddon = new ImageAddon();

  term.loadAddon(webLinksAddon);
  term.loadAddon(searchAddon);
  term.loadAddon(clipboardAddon);
  term.loadAddon(unicode11Addon);
  term.loadAddon(imageAddon);

  try {
    term.unicode.activeVersion = "11";
  } catch {
    // Non-fatal if Unicode version 11 provider is already active.
  }

  let webglAddon: WebglAddon | undefined;
  let canvasAddon: CanvasAddon | undefined;

  try {
    webglAddon = new WebglAddon();
    webglAddon.onContextLoss(() => {
      webglAddon?.dispose();
    });
    term.loadAddon(webglAddon);
  } catch {
    try {
      canvasAddon = new CanvasAddon();
      term.loadAddon(canvasAddon);
    } catch {
      // Fallback to standard DOM renderer if Canvas/WebGL unavailable
    }
  }

  return { searchAddon, webLinksAddon, clipboardAddon, unicode11Addon, imageAddon, webglAddon, canvasAddon };
}

/** Export terminal scrollback buffer as a plain text string. */
export function exportTerminalBufferToText(term: Terminal): string {
  const buffer = term.buffer.active;
  const lines: string[] = [];
  for (let i = 0; i < buffer.length; i++) {
    const line = buffer.getLine(i);
    if (line) {
      lines.push(line.translateToString(true));
    }
  }
  return lines.join("\n");
}

/** Trigger browser download for exported terminal scrollback text. */
export function downloadTerminalScrollback(term: Terminal, connectionName: string): void {
  const text = exportTerminalBufferToText(term);
  const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  const dateStr = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  a.download = `terminal-history_${connectionName.toLowerCase().replace(/[^a-z0-9_-]/g, "_")}_${dateStr}.txt`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

/** Copy/paste shortcuts, selection, and Linux WebKitGTK IME guards. */
export function attachXtermKeyHandler(
  term: Terminal,
  onOpenSearch?: () => void,
): void {
  term.attachCustomKeyEventHandler((event) => {
    if (event.isComposing) {
      return false;
    }

    const key = event.key.toLowerCase();
    const isCmdOrCtrl = event.ctrlKey || event.metaKey;

    // Ctrl+F / Cmd+F -> Open in-terminal search
    if (isCmdOrCtrl && key === "f" && !event.shiftKey && onOpenSearch) {
      if (event.type === "keydown") {
        onOpenSearch();
      }
      return false;
    }

    // Ctrl+Shift+A / Cmd+A -> Select All
    if (isCmdOrCtrl && key === "a" && (event.shiftKey || !isCmdOrCtrl)) {
      if (event.type === "keydown") {
        term.selectAll();
      }
      return false;
    }

    // Ctrl+C / Cmd+C (when text is selected) or Ctrl+Shift+C -> Copy
    if (
      (isCmdOrCtrl && key === "c" && term.hasSelection()) ||
      (isCmdOrCtrl && event.shiftKey && key === "c")
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

    // Ctrl+V / Cmd+V or Ctrl+Shift+V or Shift+Insert -> Paste
    if (
      (isCmdOrCtrl && key === "v") ||
      (isCmdOrCtrl && event.shiftKey && key === "v") ||
      (event.shiftKey && event.key === "Insert")
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

/** Attach mouse listeners for middle-click paste and smooth selection handling. */
export function attachXtermMouseHandlers(
  container: HTMLElement,
  term: Terminal,
): () => void {
  const handleAuxClick = (e: MouseEvent) => {
    if (e.button === 1) { // Middle click paste
      e.preventDefault();
      navigator.clipboard.readText().then((text) => {
        if (text) term.paste(text);
      }).catch(() => {});
    }
  };

  container.addEventListener("auxclick", handleAuxClick);
  return () => {
    container.removeEventListener("auxclick", handleAuxClick);
  };
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
