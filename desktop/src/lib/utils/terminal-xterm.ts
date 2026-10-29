import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import {
  BrowserClipboardProvider,
  ClipboardAddon,
  type ClipboardSelectionType,
  type IClipboardProvider,
} from "@xterm/addon-clipboard";
import { WebglAddon } from "@xterm/addon-webgl";
import { ImageAddon } from "@xterm/addon-image";
import { Unicode11Addon } from "@xterm/addon-unicode11";

import { attachOrphanCompositionEndGuard } from "$lib/utils/terminal-composition-guard";
import { TERMINAL_SHORTCUTS, type Combo } from "$lib/shortcuts";
import { enableSnapshot } from "$lib/utils/terminal-snapshot";
import { attachXtermSignals, type TerminalSignalHandlers } from "$lib/utils/terminal-signals";
import { attachTerminalContextMenu } from "$lib/utils/terminal-context-menu";
import {
  attachPasteGuard,
  attachMiddleClickPaste,
  pasteIntoTerminal,
} from "$lib/utils/terminal-paste";

export type PtyChunk = { data: string; bytes: number };
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

/** Tell the backend PTY the terminal's current size. */
export function syncPtySize(sessionId: string, term: Terminal): void {
  if (term.cols <= 0 || term.rows <= 0) return;
  invoke("resize_pty", { sessionId, cols: term.cols, rows: term.rows }).catch(() => {});
}

/**
 * Forward real grid changes to the PTY. `FitAddon.fit()` only resizes when
 * the proposed cols/rows differ, so the remote side gets a SIGWINCH (and a
 * full-screen redraw) only when the grid actually changed.
 */
export function attachXtermPtyResize(sessionId: string, term: Terminal): void {
  term.onResize(() => syncPtySize(sessionId, term));
}

let fontWait: Promise<void> | null = null;

/**
 * Resolve once the terminal font is loaded (or after a short timeout), so
 * xterm measures its cell size with the real font on `term.open`. If the font
 * only arrives after the timeout, `onLateLoad` lets callers re-measure.
 */
export function waitForTerminalFont(fontFamily: string, onLateLoad?: () => void): Promise<void> {
  if (fontWait) return fontWait;
  if (typeof document === "undefined" || !document.fonts) return Promise.resolve();
  let timedOut = false;
  const load = document.fonts
    .load(`13px ${fontFamily}`)
    .then(() => {
      if (timedOut) onLateLoad?.();
    })
    .catch(() => {});
  const timeout = new Promise<void>((resolve) =>
    setTimeout(() => {
      timedOut = true;
      resolve();
    }, 1200),
  );
  fontWait = Promise.race([load, timeout]);
  return fontWait;
}

/**
 * OSC 52 clipboard provider that lets the remote side set the clipboard but
 * never read it. The default provider answers `OSC 52 ; c ; ?` with the local
 * clipboard contents, so any remote host (or a file being `cat`ed) could
 * exfiltrate whatever the user last copied, e.g. a password.
 */
class WriteOnlyClipboardProvider implements IClipboardProvider {
  private readonly inner = new BrowserClipboardProvider();

  readText(): string {
    return "";
  }

  writeText(selection: ClipboardSelectionType, text: string): Promise<void> {
    return this.inner.writeText(selection, text).catch(() => {});
  }
}

export interface LoadedAddons {
  searchAddon: SearchAddon;
  webLinksAddon: WebLinksAddon;
  clipboardAddon: ClipboardAddon;
  unicode11Addon: Unicode11Addon;
  imageAddon: ImageAddon;
  webglAddon?: WebglAddon;
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
  const clipboardAddon = new ClipboardAddon(undefined, new WriteOnlyClipboardProvider());
  const unicode11Addon = new Unicode11Addon();
  // Cap decoded image memory per terminal (default is 128 MB).
  const imageAddon = new ImageAddon({ storageLimit: 16 });

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

  return { searchAddon, webLinksAddon, clipboardAddon, unicode11Addon, imageAddon };
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

let smartCopy = false;

/**
 * "Smart copy": Ctrl+C copies while text is selected instead of sending
 * SIGINT. Off by default so plain Ctrl+C always interrupts the shell.
 */
export function setTerminalSmartCopy(enabled: boolean): void {
  smartCopy = enabled;
}

const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || "");

/** Does the key event press the key named last in a shortcuts.ts combo? */
function keyMatches(event: KeyboardEvent, key: string): boolean {
  switch (key) {
    case "=":
      return event.code === "Equal" || event.code === "NumpadAdd" || event.key === "=" || event.key === "+";
    case "-":
      return event.code === "Minus" || event.code === "NumpadSubtract" || event.key === "-" || event.key === "_";
    case "0":
      return event.code === "Digit0" || event.code === "Numpad0" || event.key === "0" || event.key === ")";
    case "Esc":
      return event.key === "Escape";
    default:
      return event.key.toLowerCase() === key.toLowerCase();
  }
}

function comboMatches(event: KeyboardEvent, combo: Combo): boolean {
  const key = combo[combo.length - 1];
  const mods = combo.slice(0, -1);
  if (event.ctrlKey !== mods.includes("Ctrl")) return false;
  if (event.metaKey !== mods.includes("⌘")) return false;
  if (event.altKey !== mods.includes("Alt")) return false;
  // "=" needs Shift on most layouts, so Cmd+= and Cmd++ are the same chord.
  if (key !== "=" && event.shiftKey !== mods.includes("Shift")) return false;
  return keyMatches(event, key);
}

/** Terminal shortcuts the tab store runs itself (copy, paste and find stay in the key handler). */
const STORE_SHORTCUT_IDS = new Set([
  "term-next-tab",
  "term-prev-tab",
  "term-close-tab",
  "term-reopen-tab",
  "term-zoom-in",
  "term-zoom-out",
  "term-zoom-reset",
  "term-cancel",
]);

/** The id of the tab, zoom or cancel shortcut the event presses, if any. */
export function terminalShortcutId(event: KeyboardEvent): string | null {
  for (const shortcut of TERMINAL_SHORTCUTS) {
    if (!STORE_SHORTCUT_IDS.has(shortcut.id)) continue;
    if (shortcut.combos.some((combo) => comboMatches(event, combo))) return shortcut.id;
  }
  return null;
}

/** True when the event comes from an xterm input textarea. */
export function isXtermEvent(event: Event): boolean {
  const target = event.target;
  return target instanceof HTMLElement && target.classList.contains("xterm-helper-textarea");
}

/** The shortcut that opens the command palette from inside a terminal. */
export function isTerminalPaletteKey(event: KeyboardEvent): boolean {
  const key = event.key.toLowerCase();
  if (IS_MAC) return event.metaKey && !event.shiftKey && !event.altKey && key === "k";
  return event.ctrlKey && event.shiftKey && !event.altKey && key === "p";
}

/**
 * Terminal keys. Plain Ctrl+C/V/F/K always reach the shell; the app's
 * copy, paste and find are Ctrl+Shift+C/V/F (Cmd+C/V/F on macOS).
 * Keys returned `false` here are not sent to the PTY and still bubble to the
 * window, where the palette shortcut is handled. Tab, zoom and cancel
 * shortcuts run through `onShortcut` and stop there.
 */
export function attachXtermKeyHandler(
  term: Terminal,
  onOpenSearch?: () => void,
  /** Runs a tab/zoom/cancel shortcut; false means it did not apply (Esc with nothing connecting). */
  onShortcut?: (id: string) => boolean,
): void {
  const copySelection = () => {
    const selection = term.getSelection();
    if (!selection) return;
    copyTextWithFallback(selection);
    term.clearSelection();
  };

  const pasteText = () => {
    const doPaste = (text: string) => {
      if (text) void pasteIntoTerminal(term, text);
    };
    if (typeof navigator.clipboard?.readText === "function") {
      navigator.clipboard.readText().then(doPaste).catch((err) => {
        console.error("Failed to read from clipboard", err);
      });
    }
  };

  /** Run `action` once per key press and keep the browser from acting too. */
  const consume = (event: KeyboardEvent, action: () => void): false => {
    if (event.type === "keydown") {
      event.preventDefault();
      action();
    }
    return false;
  };

  term.attachCustomKeyEventHandler((event) => {
    if (event.isComposing) {
      return false;
    }

    const key = event.key.toLowerCase();
    // The app modifier: Cmd on macOS, Ctrl+Shift elsewhere.
    const appMod = IS_MAC ? event.metaKey && !event.ctrlKey : event.ctrlKey && event.shiftKey;
    const noAlt = !event.altKey;

    if (isTerminalPaletteKey(event)) return false;
    // Ctrl/Cmd+, opens Settings; let it bubble to the window like the palette key.
    if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && (key === "," || event.code === "Comma")) {
      return false;
    }

    const shortcutId = onShortcut ? terminalShortcutId(event) : null;
    if (shortcutId) {
      if (event.type !== "keydown") return shortcutId === "term-cancel";
      if (onShortcut!(shortcutId)) {
        event.preventDefault();
        // Keep the page-level key handlers from acting on it a second time.
        event.stopPropagation();
        return false;
      }
      return true;
    }

    if (appMod && noAlt && key === "f" && !(IS_MAC && event.shiftKey)) {
      return onOpenSearch ? consume(event, onOpenSearch) : true;
    }

    // Ctrl+Shift+A / Cmd+A -> Select all (plain Ctrl+A stays with the shell)
    if (appMod && noAlt && key === "a") {
      return consume(event, () => term.selectAll());
    }

    if (appMod && noAlt && key === "c") {
      return consume(event, copySelection);
    }
    // Opt-in: plain Ctrl+C copies while text is selected.
    if (smartCopy && !IS_MAC && event.ctrlKey && !event.shiftKey && noAlt && key === "c" && term.hasSelection()) {
      return consume(event, copySelection);
    }

    if (
      (appMod && noAlt && key === "v") ||
      (event.shiftKey && !event.ctrlKey && !event.metaKey && event.key === "Insert")
    ) {
      return consume(event, pasteText);
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
  /** Queue PTY output; it is written to the terminal on the next animation frame. */
  push: (data: string, bytes?: number) => void;
  /** Immediately write everything queued so far (e.g. before a scroll). */
  flush: () => void;
  /** Stop the flush timer and drop any queued output. */
  dispose: () => void;
}

const MAX_WRITE_CHUNK = 64 * 1024;
const MAX_QUEUE_BYTES = 2 * 1024 * 1024;

/**
 * Coalesce PTY output events into a single `term.write` per animation frame.
 *
 * The backend can emit hundreds of IPC events per second during
 * high-throughput output; writing each one to xterm individually janks the
 * UI and can crash the webview under load. This batches them while keeping
 * ordering intact and drops cleanly once the terminal is disposed.
 *
 * Uses `requestAnimationFrame` instead of `setTimeout` so flushes are
 * aligned with the browser's repaint cycle — reducing dropped frames and
 * giving the smoothest possible scroll experience.
 */
export function createOutputCoalescer(
  term: Terminal,
  options?: { onFlush?: () => void; onAck?: (bytes: number) => void },
): OutputCoalescer {
  let pending = "";
  let pendingBytes = 0;
  let rafId: number | null = null;
  let disposed = false;

  const isTermAlive = () => {
    try {
      return !(term as Terminal & { isDisposed?: boolean }).isDisposed;
    } catch {
      return false;
    }
  };

  const writeChunked = (data: string, bytes: number) => {
    if (disposed || !isTermAlive()) {
      if (bytes > 0) options?.onAck?.(bytes);
      return;
    }
    if (data.length <= MAX_WRITE_CHUNK) {
      term.write(data, () => {
        options?.onFlush?.();
        if (bytes > 0) options?.onAck?.(bytes);
      });
      return;
    }
    let offset = 0;
    const writeNext = () => {
      if (disposed || !isTermAlive()) {
        if (bytes > 0) options?.onAck?.(bytes);
        return;
      }
      const chunk = data.slice(offset, offset + MAX_WRITE_CHUNK);
      if (!chunk) {
        options?.onFlush?.();
        if (bytes > 0) options?.onAck?.(bytes);
        return;
      }
      offset += MAX_WRITE_CHUNK;
      term.write(chunk, writeNext);
    };
    writeNext();
  };

  const flush = () => {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    if (disposed || (!pending && pendingBytes === 0)) return;
    const data = pending;
    const bytes = pendingBytes;
    pending = "";
    pendingBytes = 0;
    writeChunked(data, bytes);
  };

  return {
    push(data: string, bytes = 0) {
      if (disposed || (!data && bytes <= 0)) {
        if (disposed && bytes > 0) options?.onAck?.(bytes);
        return;
      }
      pending += data;
      pendingBytes += bytes;
      // If queue is dangerously large, flush immediately to prevent OOM.
      if (pending.length >= MAX_QUEUE_BYTES) {
        flush();
        return;
      }
      if (rafId === null) {
        rafId = requestAnimationFrame(() => {
          rafId = null;
          flush();
        });
      }
    },
    flush,
    dispose() {
      disposed = true;
      if (pendingBytes > 0) {
        options?.onAck?.(pendingBytes);
      }
      pending = "";
      pendingBytes = 0;
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    },
  };
}

export interface XtermSessionOptions {
  terminalOptions?: Record<string, unknown>;
  onOpenSearch?: () => void;
  onShortcut?: (id: string) => boolean;
  signals?: TerminalSignalHandlers;
  onZoom?: (delta: 1 | -1) => void;
  getConnectionName?: () => string;
}

export interface XtermSessionHandle {
  term: Terminal;
  fitAddon: FitAddon;
  searchAddon: SearchAddon;
  setVisible: (visible: boolean) => void;
  write: (data: string, bytes: number) => void;
  dispose: () => void;
}

/**
 * Shared factory for xterm sessions (main tabs and popout windows).
 * Wires addons, snapshotting, signals, context menu, paste guard, middle-click paste,
 * Linux input fix, shortcuts, keyboard handling, zoom, resize and IO.
 */
export function createXtermSession(
  container: HTMLElement,
  sessionId: string,
  options?: XtermSessionOptions,
): XtermSessionHandle {
  const term = new Terminal(options?.terminalOptions as ConstructorParameters<typeof Terminal>[0]);
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);

  const { searchAddon } = attachXtermAddons(term);
  enableSnapshot(term);

  term.open(container);

  let disposed = false;
  let webglAddon: WebglAddon | undefined;
  let webglFailed = false;

  const setVisible = (visible: boolean) => {
    if (disposed) return;
    if (visible) {
      if (!webglAddon && !webglFailed) {
        try {
          const addon = new WebglAddon();
          addon.onContextLoss(() => {
            addon.dispose();
            if (webglAddon === addon) {
              webglAddon = undefined;
            }
            webglFailed = true;
          });
          term.loadAddon(addon);
          webglAddon = addon;
        } catch {
          webglFailed = true;
        }
      }
    } else {
      if (webglAddon) {
        try {
          webglAddon.dispose();
        } catch {}
        webglAddon = undefined;
      }
    }
  };

  const cleanupLinuxFix = attachXtermLinuxInputFix(container, term);

  const cleanupContextMenu = attachTerminalContextMenu(container, term, {
    paste: () => {
      const doPaste = (text: string) => {
        if (text) void pasteIntoTerminal(term, text);
      };
      if (typeof navigator.clipboard?.readText === "function") {
        navigator.clipboard.readText().then(doPaste).catch(() => {});
      }
    },
    find: () => {
      options?.onOpenSearch?.();
    },
    exportScrollback: () => {
      const name = options?.getConnectionName ? options.getConnectionName() : "terminal";
      downloadTerminalScrollback(term, name);
    },
  });

  const cleanupPasteGuard = attachPasteGuard(container, term);
  const cleanupMiddleClick = attachMiddleClickPaste(container, term);

  let cleanupSignals: (() => void) | undefined;
  if (options?.signals) {
    cleanupSignals = attachXtermSignals(term, {
      onTitle: (title) => options.signals?.onTitle(title),
      onBell: () => options.signals?.onBell(),
      onProgress: (state, value) => options.signals?.onProgress(state, value),
    });
  }

  attachXtermKeyHandler(term, options?.onOpenSearch, options?.onShortcut);
  attachXtermPtyResize(sessionId, term);
  attachXtermIo(sessionId, term);

  let fitRaf: number | null = null;
  const scheduleFit = () => {
    if (disposed || fitRaf !== null) return;
    fitRaf = requestAnimationFrame(() => {
      fitRaf = null;
      if (
        disposed ||
        !container.isConnected ||
        container.offsetParent === null ||
        container.clientWidth === 0 ||
        container.clientHeight === 0
      ) {
        return;
      }
      try {
        fitAddon.fit();
      } catch {}
    });
  };

  const resizeObserver = new ResizeObserver(() => scheduleFit());
  resizeObserver.observe(container);

  const onMouseDown = () => {
    try {
      term.focus();
    } catch {}
  };
  container.addEventListener("mousedown", onMouseDown);

  const onWheel = (e: WheelEvent) => {
    if ((e.ctrlKey || e.metaKey) && options?.onZoom) {
      e.preventDefault();
      const delta: 1 | -1 = e.deltaY < 0 ? 1 : -1;
      options.onZoom(delta);
    }
  };
  container.addEventListener("wheel", onWheel, { passive: false });

  const coalescer = createOutputCoalescer(term, {
    onAck: (bytes) => {
      if (bytes > 0) {
        invoke("ack_pty", { sessionId, bytes }).catch(() => {});
      }
    },
  });

  const write = (data: string, bytes: number) => {
    if (disposed) {
      if (bytes > 0) {
        invoke("ack_pty", { sessionId, bytes }).catch(() => {});
      }
      return;
    }
    coalescer.push(data, bytes);
  };

  const dispose = () => {
    if (disposed) return;
    disposed = true;

    setVisible(false);
    coalescer.dispose();

    container.removeEventListener("wheel", onWheel);
    container.removeEventListener("mousedown", onMouseDown);
    resizeObserver.disconnect();
    if (fitRaf !== null) {
      cancelAnimationFrame(fitRaf);
      fitRaf = null;
    }

    cleanupLinuxFix();
    cleanupContextMenu();
    cleanupPasteGuard();
    cleanupMiddleClick();
    cleanupSignals?.();

    try {
      term.dispose();
    } catch {}
  };

  return {
    term,
    fitAddon,
    searchAddon,
    setVisible,
    write,
    dispose,
  };
}
