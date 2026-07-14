import { tick } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";

import { getCurrentXtermTheme } from "$lib/utils/theme";
import { isTerminalFocused } from "$lib/utils/terminal-focus";
import {
  attachXtermIo,
  attachXtermKeyHandler,
  attachXtermLinuxInputFix,
} from "$lib/utils/terminal-xterm";

export interface PopoutTerminalHandle {
  connectionName: string;
  releaseUi: () => void;
  shutdown: (options?: { closeWindow?: boolean }) => Promise<void>;
}

export async function initPopoutTerminal(sessionId: string): Promise<PopoutTerminalHandle> {
  const windowLabel = getCurrentWindow().label;
  const info = await invoke<{
    session_id: string;
    connection_name: string;
    buffered_output: string;
  }>("claim_popout_session", { sessionId, windowLabel });

  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let unlistenOutput: UnlistenFn | null = null;
  let themeObserver: MutationObserver | null = null;
  let compositionGuardCleanup: (() => void) | null = null;
  let closing = false;
  let popoutFontSize = 13;

  const container = await waitForContainer("terminal-popout-root");
  if (!container) {
    throw new Error("Terminal container not found.");
  }

  term = new Terminal({
    cursorBlink: true,
    fontFamily: "JetBrains Mono, Courier New, monospace",
    fontSize: popoutFontSize,
    lineHeight: 1.15,
    scrollback: 5000,
    theme: getCurrentXtermTheme(),
  });

  fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(container);
  compositionGuardCleanup = attachXtermLinuxInputFix(container, term);
  term.focus();

  container.addEventListener("mousedown", () => term!.focus());

  if (info.buffered_output && term) {
    const activeTerm = term;
    await new Promise<void>((resolve) => {
      activeTerm.write(info.buffered_output, () => {
        activeTerm.scrollToBottom();
        resolve();
      });
    });
  } else if (term) {
    const activeTerm = term;
    requestAnimationFrame(() => activeTerm.scrollToBottom());
  }

  attachXtermIo(sessionId, term);
  attachXtermKeyHandler(term);

  const fit = () => {
    if (!term || !fitAddon) return;
    try {
      fitAddon.fit();
      invoke("resize_pty", {
        sessionId,
        cols: term.cols,
        rows: term.rows,
      }).catch(() => {});
    } catch {
      // Container may not have dimensions yet.
    }
  };

  // Dynamic theme mutation observer
  themeObserver = new MutationObserver(() => {
    if (term) {
      term.options.theme = getCurrentXtermTheme();
    }
  });
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });

  // Ctrl + Mouse Wheel zoom
  container.addEventListener("wheel", (e) => {
    if (e.ctrlKey) {
      e.preventDefault();
      const nextSize = e.deltaY < 0 ? popoutFontSize + 1 : popoutFontSize - 1;
      popoutFontSize = Math.max(8, Math.min(32, nextSize));
      if (term) {
        term.options.fontSize = popoutFontSize;
        fit();
      }
    }
  }, { passive: false });

  // Keyboard shortcut zoom — skip when xterm has focus so vim/nano keys aren't stolen
  const handleKeydown = (e: KeyboardEvent) => {
    if (isTerminalFocused()) return;
    if (e.ctrlKey && (e.key === "=" || e.key === "+")) {
      e.preventDefault();
      popoutFontSize = Math.max(8, Math.min(32, popoutFontSize + 1));
      if (term) {
        term.options.fontSize = popoutFontSize;
        fit();
      }
    } else if (e.ctrlKey && e.key === "-") {
      e.preventDefault();
      popoutFontSize = Math.max(8, Math.min(32, popoutFontSize - 1));
      if (term) {
        term.options.fontSize = popoutFontSize;
        fit();
      }
    } else if (e.ctrlKey && e.key === "0") {
      e.preventDefault();
      popoutFontSize = 13;
      if (term) {
        term.options.fontSize = popoutFontSize;
        fit();
      }
    }
  };
  window.addEventListener("keydown", handleKeydown);

  resizeObserver = new ResizeObserver(() => fit());
  resizeObserver.observe(container);
  requestAnimationFrame(() => requestAnimationFrame(fit));

  unlistenOutput = await listen("pty-output", (event) => {
    const payload = event.payload as { session_id?: string; sessionId?: string; data: string };
    const id = payload.session_id ?? payload.sessionId;
    if (id !== sessionId || !term) return;
    const wasAtBottom = term.buffer.active.baseY + term.rows >= term.buffer.active.length;
    term.write(payload.data, () => {
      if (wasAtBottom) term?.scrollToBottom();
    });
  });

  const releaseUi = () => {
    window.removeEventListener("keydown", handleKeydown);
    compositionGuardCleanup?.();
    compositionGuardCleanup = null;
    themeObserver?.disconnect();
    themeObserver = null;
    unlistenOutput?.();
    unlistenOutput = null;
    resizeObserver?.disconnect();
    resizeObserver = null;
    term?.dispose();
    term = null;
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
    releaseUi,
    shutdown,
  };
}

export async function dockPopoutToMain(sessionId: string): Promise<void> {
  const windowLabel = getCurrentWindow().label;
  await invoke("dock_popout_session", { sessionId, windowLabel });
}

export interface PopoutMainOverlap {
  overlaps: boolean;
  overlap_ratio: number;
  center_over_main: boolean;
  should_dock: boolean;
}

export async function checkPopoutMainOverlap(
  windowLabel: string,
): Promise<PopoutMainOverlap> {
  return invoke<PopoutMainOverlap>("check_popout_main_overlap", {
    popoutWindowLabel: windowLabel,
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
