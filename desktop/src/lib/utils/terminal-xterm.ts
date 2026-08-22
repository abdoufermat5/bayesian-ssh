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

/**
 * Write to a terminal without throwing if it has been disposed while data
 * was in flight (e.g. a tab closed right as output arrived). Writing to a
 * disposed xterm instance throws and can crash the store's event handler.
 */
export function safeWrite(term: Terminal | undefined, data: string, callback?: () => void): void {
  if (!term) return;
  try {
    if ((term as Terminal & { isDisposed?: boolean }).isDisposed) return;
    term.write(data, callback);
  } catch {
    // Terminal was disposed concurrently — drop the output.
  }
}

/** Instantiates and loads web-links, search, clipboard, unicode11, image, and GPU acceleration addons onto the terminal. */
export function attachXtermAddons(term: Terminal): LoadedAddons {
  // Only allow http/https links to reach the system browser. Terminal output
  // is untrusted input — `file:`, `smb:` or other schemes must never launch.
  const webLinksAddon = new WebLinksAddon((event, uri) => {
    let parsed: URL;
    try {
      parsed = new URL(uri);
    } catch {
      return;
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return;
    }
    event.preventDefault();
    openUrl(uri).catch(() => {
      window.open(uri, "_blank", "noopener,noreferrer");
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

  const installCanvasFallback = () => {
    if (canvasAddon) return;
    try {
      canvasAddon = new CanvasAddon();
      term.loadAddon(canvasAddon);
    } catch {
      // Fall back to standard DOM renderer if Canvas/WebGL unavailable.
    }
  };

  try {
    webglAddon = new WebglAddon();
    webglAddon.onContextLoss(() => {
      // A lost WebGL context would otherwise leave a permanently blank
      // terminal — dispose it and switch to the canvas renderer.
      webglAddon?.dispose();
      webglAddon = undefined;
      installCanvasFallback();
    });
    term.loadAddon(webglAddon);
  } catch {
    installCanvasFallback();
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

/**
 * Copy text to the clipboard with a WebKitGTK-safe fallback (Tauri's Linux
 * webview can reject `navigator.clipboard` outside of user-gesture paths).
 */
export function copyTextWithFallback(text: string): void {
  if (!text) return;
  const legacyFallback = () => {
    // Legacy fallback: hidden textarea + execCommand (still the only
    // reliable path on some WebKitGTK builds).
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    try {
      document.execCommand("copy");
    } catch (err) {
      console.error("Failed to copy to clipboard", err);
    } finally {
      document.body.removeChild(textarea);
    }
  };
  if (typeof navigator.clipboard?.writeText === "function") {
    navigator.clipboard.writeText(text).catch(legacyFallback);
    return;
  }
  legacyFallback();
}

/** Copy/paste shortcuts, selection, and Linux WebKitGTK IME guards. */
export function attachXtermKeyHandler(
  term: Terminal,
  onOpenSearch?: () => void,
): void {
  const copySelection = () => {
    const selection = term.getSelection();
    if (!selection) return;
    copyTextWithFallback(selection);
    term.clearSelection();
  };

  const pasteText = () => {
    const doPaste = (text: string) => {
      if (text) term.paste(text);
    };
    navigator.clipboard.readText().then(doPaste).catch((err) => {
      console.error("Failed to read from clipboard", err);
    });
  };

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
        copySelection();
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
        pasteText();
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
  const deliverOrphan = (data: string) => {
    try {
      if ((term as Terminal & { isDisposed?: boolean }).isDisposed) return;
      term.input(data, true);
    } catch {
      // Terminal disposed concurrently — drop the keystroke.
    }
  };
  return attachOrphanCompositionEndGuard(container, deliverOrphan);
}

export interface OutputCoalescer {
  /** Queue PTY output; it is written to the terminal on the next flush. */
  push: (data: string) => void;
  /** Immediately write everything queued so far (e.g. before a scroll). */
  flush: () => void;
  /** Stop the flush timer and drop any queued output. */
  dispose: () => void;
}

const MAX_WRITE_CHUNK = 64 * 1024;

/**
 * Coalesce PTY output events into a single `term.write` per animation frame.
 *
 * The backend can emit hundreds of IPC events per second during
 * high-throughput output; writing each one to xterm individually janks the
 * UI and can crash the webview under load. This batches them while keeping
 * ordering intact and drops cleanly once the terminal is disposed.
 */
export function createOutputCoalescer(
  term: Terminal,
  options?: { onFlush?: () => void },
): OutputCoalescer {
  let pending: string[] = [];
  let pendingBytes = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let disposed = false;

  const flush = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (disposed || pending.length === 0) return;
    const chunks = pending;
    pending = [];
    pendingBytes = 0;

    const writeChunk = (index: number) => {
      if (disposed) return;
      const data = chunks[index];
      if (!data) {
        options?.onFlush?.();
        return;
      }
      try {
        if ((term as Terminal & { isDisposed?: boolean }).isDisposed) return;
        term.write(data, () => {
          writeChunk(index + 1);
        });
      } catch {
        // Terminal disposed concurrently.
      }
    };
    writeChunk(0);
  };

  return {
    push(data: string) {
      if (disposed || !data) return;
      pending.push(data);
      pendingBytes += data.length;
      if (!timer) {
        // ~2 frames of latency — imperceptible interactively, a 30-60x
        // reduction in IPC decode + render work under heavy output.
        timer = setTimeout(flush, 32);
      }
      // If a single burst is huge, write it in bounded pieces immediately
      // instead of letting the queue grow without limit.
      if (pendingBytes >= 256 * 1024) {
        flush();
      }
    },
    flush,
    dispose() {
      disposed = true;
      pending = [];
      pendingBytes = 0;
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
    },
  };
}
