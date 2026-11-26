import { invoke } from "@tauri-apps/api/core";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import type { SearchAddon } from "@xterm/addon-search";
import { getTerminalSettings } from "$lib/utils/terminal-options";
import type { XtermSessionHandle } from "$lib/utils/terminal-xterm";

/** Non-reactive per-tab runtime: xterm objects, buffers and cleanups. */
export interface TabRuntime {
  session?: XtermSessionHandle;
  term?: Terminal;
  fitAddon?: FitAddon;
  searchAddon?: SearchAddon;
  ensureSearchAddon?: () => Promise<SearchAddon | undefined>;
  /** Output received while the tab is hidden or still being created. */
  pending: Array<{ data: string; bytes: number }>;
  pendingBytes: number;
  fitRaf: number | null;
  /** Cursor blink turned off while the Terminals view is hidden. */
  blinkSuspended: boolean;
  /** Plain-text tail of ssh's output, kept to explain a failed connect. */
  recentOutput: string;
}

export const runtimes = new Map<string, TabRuntime>();

/** Cap on output buffered for a hidden tab before it is handed to xterm. */
export const MAX_PENDING_OUTPUT_BYTES = 256 * 1024;

export function flushRuntime(rt: TabRuntime) {
  if (!rt.session || rt.pending.length === 0) return;
  const chunks = rt.pending;
  rt.pending = [];
  rt.pendingBytes = 0;
  for (const chunk of chunks) {
    rt.session.write(chunk.data, chunk.bytes);
  }
}

export function createRuntime(tabId: string): TabRuntime {
  const existing = runtimes.get(tabId);
  if (existing) return existing;
  const rt: TabRuntime = {
    pending: [],
    pendingBytes: 0,
    fitRaf: null,
    blinkSuspended: false,
    recentOutput: "",
  };
  runtimes.set(tabId, rt);
  return rt;
}

export function disposeRuntime(tabId: string) {
  const rt = runtimes.get(tabId);
  if (!rt) return;
  runtimes.delete(tabId);
  if (rt.fitRaf !== null) cancelAnimationFrame(rt.fitRaf);
  for (const chunk of rt.pending) {
    if (chunk.bytes > 0) {
      invoke("ack_pty", { sessionId: tabId, bytes: chunk.bytes }).catch(() => {});
    }
  }
  rt.pending = [];
  rt.pendingBytes = 0;
  rt.session?.dispose();
}

export function isContainerVisible(container: HTMLElement | null | undefined): boolean {
  if (!container) return false;
  // display:none (hidden tab/view) or detached from the document → skip.
  return container.isConnected && container.offsetParent !== null;
}

/**
 * Fit a tab's terminal at most once per animation frame. `fit()` only
 * resizes when cols/rows change, and the PTY is told via `term.onResize`,
 * so unchanged layouts never reach the backend.
 */
export function scheduleFit(tabId: string) {
  const rt = runtimes.get(tabId);
  if (!rt?.term || !rt.fitAddon || rt.fitRaf !== null) return;
  rt.fitRaf = requestAnimationFrame(() => {
    rt.fitRaf = null;
    const term = rt.term;
    if (!term || !rt.fitAddon || !isContainerVisible(term.element?.parentElement)) return;
    try {
      rt.fitAddon.fit();
    } catch {
      // Container may not have dimensions yet.
    }
  });
}

export function desiredCursorBlink(): boolean {
  return getTerminalSettings()?.terminal_cursor_blink ?? true;
}

export function setBlinkSuspended(rt: TabRuntime, suspend: boolean) {
  const term = rt.term;
  if (!term) return;
  if (suspend) {
    if (!rt.blinkSuspended && term.options.cursorBlink) {
      term.options.cursorBlink = false;
      rt.blinkSuspended = true;
    }
  } else if (rt.blinkSuspended) {
    term.options.cursorBlink = desiredCursorBlink();
    rt.blinkSuspended = false;
  }
}

/** Re-measure open terminals when the font finished loading after timeout. */
export function remeasureFonts() {
  for (const [id, rt] of runtimes) {
    const term = rt.term;
    if (!term) continue;
    const family = term.options.fontFamily;
    term.options.fontFamily = "monospace";
    term.options.fontFamily = family;
    scheduleFit(id);
  }
}
