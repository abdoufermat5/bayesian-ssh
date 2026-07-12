import { tick } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";

const XTERM_THEME = {
  background: "#0c0d12",
  foreground: "#cbd5e1",
  cursor: "#00f0ff",
  cursorAccent: "#0c0d12",
  cyan: "#00f0ff",
  magenta: "#d946ef",
  green: "#10b981",
  red: "#ef4444",
};

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
  let closing = false;

  const container = await waitForContainer("terminal-popout-root");
  if (!container) {
    throw new Error("Terminal container not found.");
  }

  term = new Terminal({
    cursorBlink: true,
    fontFamily: "JetBrains Mono, Courier New, monospace",
    fontSize: 13,
    lineHeight: 1,
    scrollback: 5000,
    theme: XTERM_THEME,
  });

  fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(container);

  if (info.buffered_output) {
    term.write(info.buffered_output);
  }

  term.onData((data) => {
    invoke("write_pty", { sessionId, data }).catch(() => {});
  });

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

async function waitForContainer(elementId: string): Promise<HTMLElement | null> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const container = document.getElementById(elementId);
    if (container) return container;
    await tick();
  }
  return null;
}
