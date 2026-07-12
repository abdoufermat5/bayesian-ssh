import { tick } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { Connection } from "$lib/types";

export interface TerminalTab {
  id: string;
  name: string;
  connectionName: string;
  term?: Terminal;
  fitAddon?: FitAddon;
}

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

let tabs = $state<TerminalTab[]>([]);
let activeTabId = $state<string | null>(null);
let listenersReady = false;
let unlistenOutput: UnlistenFn | null = null;
let unlistenExit: UnlistenFn | null = null;
const resizeObservers = new Map<string, ResizeObserver>();

type ExitCallback = (sessionId: string) => void | Promise<void>;

let onSessionExit: ExitCallback | null = null;

function findTab(tabId: string): TerminalTab | undefined {
  return tabs.find((t) => t.id === tabId);
}

function linkTerminal(tabId: string, term: Terminal, fitAddon: FitAddon) {
  const index = tabs.findIndex((t) => t.id === tabId);
  if (index !== -1) {
    tabs[index].term = term;
    tabs[index].fitAddon = fitAddon;
  }
}

function fitTerminal(tabId: string, term: Terminal, fitAddon: FitAddon) {
  try {
    fitAddon.fit();
    invoke("resize_pty", {
      sessionId: tabId,
      cols: term.cols,
      rows: term.rows,
    }).catch(() => {});
  } catch {
    // Container may not have dimensions yet.
  }
}

function createTerminal(tabId: string, container: HTMLElement, conn: Connection): Terminal {
  const term = new Terminal({
    cursorBlink: true,
    fontFamily: "JetBrains Mono, Courier New, monospace",
    fontSize: 13,
    lineHeight: 1.25,
    scrollback: 5000,
    theme: XTERM_THEME,
  });

  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(container);
  linkTerminal(tabId, term, fitAddon);

  attachResizeObserver(tabId, container, term, fitAddon);

  term.writeln(
    `\x1b[1;36mInitializing Bayesian-SSH Shell Session to ${conn.name}... \x1b[0m`,
  );

  return term;
}

function attachResizeObserver(tabId: string, container: HTMLElement, term: Terminal, fitAddon: FitAddon) {
  const observer = new ResizeObserver(() => {
    fitTerminal(tabId, term, fitAddon);
  });
  observer.observe(container);
  resizeObservers.set(tabId, observer);

  requestAnimationFrame(() => {
    requestAnimationFrame(() => fitTerminal(tabId, term, fitAddon));
  });
}

function detachResizeObserver(tabId: string) {
  resizeObservers.get(tabId)?.disconnect();
  resizeObservers.delete(tabId);
}

/** Re-fit all terminals — e.g. when switching back to the Terminals tab. */
export function fitActiveTerminal() {
  for (const tab of tabs) {
    if (tab.term && tab.fitAddon) {
      fitTerminal(tab.id, tab.term, tab.fitAddon);
    }
  }
}

export async function initTerminalListeners(onExit?: ExitCallback) {
  if (listenersReady) return;
  listenersReady = true;
  onSessionExit = onExit ?? null;

  unlistenOutput = await listen("pty-output", (event) => {
    const payload = event.payload as { session_id: string; data: string };
    findTab(payload.session_id)?.term?.write(payload.data);
  });

  unlistenExit = await listen("pty-exit", (event) => {
    const sessionId = event.payload as string;
    const tab = findTab(sessionId);
    tab?.term?.writeln("\n\x1b[1;33mSession disconnected.\x1b[0m");

    setTimeout(() => {
      disconnectTab(sessionId);
    }, 800);

    onSessionExit?.(sessionId);
  });
}

export async function teardownTerminalListeners() {
  unlistenOutput?.();
  unlistenExit?.();
  unlistenOutput = null;
  unlistenExit = null;
  listenersReady = false;
}

export async function connectSSH(conn: Connection): Promise<void> {
  const tabId = crypto.randomUUID().replace(/-/g, "").slice(0, 12);
  const newTab: TerminalTab = { id: tabId, name: conn.name, connectionName: conn.name };

  tabs = [...tabs, newTab];
  activeTabId = tabId;

  await tick();

  const container = document.getElementById(`terminal-${tabId}`);
  if (!container) return;

  const term = createTerminal(tabId, container, conn);

  try {
    await invoke("spawn_pty", { sessionId: tabId, connectionName: conn.name });

    term.onData((data) => {
      invoke("write_pty", { sessionId: tabId, data }).catch(() => {});
    });
  } catch (e: unknown) {
    term.writeln(`\n\x1b[1;31mError spawning terminal process: ${String(e)}\x1b[0m`);
  }
}

export async function disconnectTab(tabId: string): Promise<void> {
  try {
    await invoke("close_pty", { sessionId: tabId });
  } catch {
    // Session may already be closed.
  }

  const tab = findTab(tabId);
  tab?.term?.dispose();
  detachResizeObserver(tabId);

  tabs = tabs.filter((t) => t.id !== tabId);

  if (activeTabId === tabId) {
    activeTabId = tabs.length > 0 ? tabs[0].id : null;
  }
}

export async function closeAllTabs(): Promise<void> {
  for (const tab of [...tabs]) {
    await disconnectTab(tab.id);
  }
}

export function getTerminalState() {
  return {
    get tabs() {
      return tabs;
    },
    get activeTabId() {
      return activeTabId;
    },
    set activeTabId(value: string | null) {
      activeTabId = value;
    },
    get count() {
      return tabs.length;
    },
  };
}
