import { tick } from "svelte";
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { DesktopSettings } from "$lib/types";
import { getCurrentXtermTheme } from "$lib/utils/theme";
import { isTerminalFocused } from "$lib/utils/terminal-focus";
import { buildTerminalOptions } from "$lib/stores/terminal.svelte";
import {
  createXtermSession,
  safeWrite,
  syncPtySize,
  waitForTerminalFont,
  type PtyChunk,
  type XtermSessionHandle,
} from "$lib/utils/terminal-xterm";
import { snapshotTerminal } from "$lib/utils/terminal-snapshot";
import { requestAttentionIfUnfocused, setWindowProgress } from "$lib/utils/terminal-signals";

export interface PopoutTerminalHandle {
  connectionName: string;
  getSnapshot: () => string | null;
  releaseUi: () => void;
  shutdown: (options?: { closeWindow?: boolean }) => Promise<void>;
}

interface ClaimInfo {
  session_id: string;
  connection_name: string;
  buffered_output: string;
}

export async function initPopoutTerminal(
  sessionId: string,
  options?: { onExit?: () => void },
): Promise<PopoutTerminalHandle> {
  const windowLabel = getCurrentWindow().label;
  let session: XtermSessionHandle | null = null;
  let closing = false;
  let replayDone = false;
  let earlyChunks: Array<{ data: string; bytes: number }> = [];

  const outputChannel = new Channel<PtyChunk>((chunk) => {
    if (closing) {
      if (chunk.bytes > 0) {
        invoke("ack_pty", { sessionId, bytes: chunk.bytes }).catch(() => {});
      }
      return;
    }
    if (!replayDone || !session) {
      earlyChunks.push(chunk);
      return;
    }
    session.write(chunk.data, chunk.bytes);
  });

  const info = await invoke<ClaimInfo>("claim_popout_session", {
    sessionId,
    windowLabel,
    output: outputChannel,
  });

  let themeObserver: MutationObserver | null = null;

  const container = await waitForContainer("terminal-popout-root");
  if (!container) {
    for (const chunk of earlyChunks) {
      if (chunk.bytes > 0) {
        invoke("ack_pty", { sessionId, bytes: chunk.bytes }).catch(() => {});
      }
    }
    earlyChunks = [];
    throw new Error("Terminal container not found.");
  }

  // Apply saved terminal preferences (font size, family, line height, cursor style, scrollback).
  let settings: DesktopSettings | undefined;
  try {
    const loaded = await invoke<DesktopSettings>("load_desktop_settings");
    if (loaded && typeof loaded === "object") {
      settings = loaded;
    }
  } catch {
    // Keep defaults if settings are unavailable.
  }

  const termOptions = buildTerminalOptions(settings);
  // Measure cells with the real font, not a fallback.
  await waitForTerminalFont(String(termOptions.fontFamily), () => {
    if (!session) return;
    const family = session.term.options.fontFamily;
    session.term.options.fontFamily = "monospace";
    session.term.options.fontFamily = family;
    session.fitAddon.fit();
  });

  const handleZoomDelta = (delta: 1 | -1) => {
    if (!session) return;
    const current = session.term.options.fontSize ?? 13;
    const clamped = Math.max(8, Math.min(32, current + delta));
    session.term.options.fontSize = clamped;
    session.fitAddon.fit();
  };

  session = createXtermSession(container, sessionId, {
    terminalOptions: termOptions,
    getConnectionName: () => info.connection_name,
    onZoom: handleZoomDelta,
    signals: {
      onTitle: (title) => {
        const displayTitle = title ? title.trim() : info.connection_name;
        getCurrentWindow().setTitle(`${displayTitle} — Bayesian SSH`).catch(() => {});
      },
      onBell: () => {
        void requestAttentionIfUnfocused();
      },
      onProgress: (state, value) => {
        void setWindowProgress(state, value);
      },
    },
  });

  // Popout is always visible; enable WebGL
  session.setVisible(true);

  // Dynamic theme mutation observer
  themeObserver = new MutationObserver(() => {
    if (session) {
      session.term.options.theme = getCurrentXtermTheme();
    }
  });
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });

  // Keyboard shortcut zoom — skip when xterm has focus so vim/nano keys aren't stolen
  const handleKeydown = (e: KeyboardEvent) => {
    if (isTerminalFocused() || !session) return;
    if (e.ctrlKey && (e.key === "=" || e.key === "+")) {
      e.preventDefault();
      handleZoomDelta(1);
    } else if (e.ctrlKey && e.key === "-") {
      e.preventDefault();
      handleZoomDelta(-1);
    } else if (e.ctrlKey && e.key === "0") {
      e.preventDefault();
      const resetSize = settings?.terminal_font_size ?? 13;
      session.term.options.fontSize = resetSize;
      session.fitAddon.fit();
    }
  };
  window.addEventListener("keydown", handleKeydown);

  let unlistenExit: UnlistenFn | null = null;
  unlistenExit = await listen(`pty-exit:${sessionId}`, () => {
    void setWindowProgress(0, 0);
    if (session) {
      safeWrite(session.term, "\n\x1b[1;33mSession disconnected.\x1b[0m\r\n");
    }
    options?.onExit?.();
  });

  const flushEarlyChunks = () => {
    replayDone = true;
    if (session) {
      for (const chunk of earlyChunks) {
        session.write(chunk.data, chunk.bytes);
      }
    } else {
      for (const chunk of earlyChunks) {
        if (chunk.bytes > 0) {
          invoke("ack_pty", { sessionId, bytes: chunk.bytes }).catch(() => {});
        }
      }
    }
    earlyChunks = [];
  };

  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      if (!session) {
        flushEarlyChunks();
        return;
      }
      session.fitAddon.fit();
      // The PTY still has the size of the view it came from.
      syncPtySize(sessionId, session.term);
      session.term.focus();

      if (info.buffered_output) {
        safeWrite(session.term, info.buffered_output, () => {
          session?.term.scrollToBottom();
          flushEarlyChunks();
          invoke("redraw_pty", { sessionId }).catch(() => {});
        });
      } else {
        requestAnimationFrame(() => {
          session?.term.scrollToBottom();
          flushEarlyChunks();
          invoke("redraw_pty", { sessionId }).catch(() => {});
        });
      }
    }),
  );

  const releaseUi = () => {
    closing = true;
    void setWindowProgress(0, 0);
    window.removeEventListener("keydown", handleKeydown);
    themeObserver?.disconnect();
    themeObserver = null;
    unlistenExit?.();
    unlistenExit = null;
    for (const chunk of earlyChunks) {
      if (chunk.bytes > 0) {
        invoke("ack_pty", { sessionId, bytes: chunk.bytes }).catch(() => {});
      }
    }
    earlyChunks = [];
    session?.dispose();
    session = null;
  };

  const shutdown = async (options?: { closeWindow?: boolean }) => {
    if (closing) return;
    closing = true;
    releaseUi();
    try {
      await invoke("close_pty", {
        sessionId,
        closeWindow: options?.closeWindow ?? false,
      });
    } catch {
      // Session may already be closed.
    }
  };

  return {
    connectionName: info.connection_name,
    getSnapshot: () => {
      return session ? snapshotTerminal(session.term) : null;
    },
    releaseUi,
    shutdown,
  };
}

export async function dockPopoutToMain(sessionId: string, snapshot?: string | null): Promise<void> {
  const windowLabel = getCurrentWindow().label;
  await invoke("dock_popout_session", {
    sessionId,
    windowLabel,
    snapshot: snapshot ?? null,
  });
}

async function waitForContainer(elementId: string): Promise<HTMLElement | null> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const container = document.getElementById(elementId);
    if (container) return container;
    await tick();
  }
  return null;
}
