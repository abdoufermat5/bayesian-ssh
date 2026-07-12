import { tick } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { Connection } from "$lib/types";
import { ensureKerberosForConnection } from "$lib/stores/kerberos.svelte";

export interface TerminalTab {
  id: string;
  name: string;
  connectionName: string;
  term?: Terminal;
  fitAddon?: FitAddon;
}

export interface DetachedSession {
  id: string;
  name: string;
  connectionName: string;
}

export interface PopoutSession {
  id: string;
  name: string;
  connectionName: string;
  windowLabel: string;
}

interface ReattachSessionPayload {
  session_id: string;
  connection_name: string;
  buffered_output: string;
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
let detachedSessions = $state<DetachedSession[]>([]);
let popoutSessions = $state<PopoutSession[]>([]);
let activeSessionCount = $state(0);
let activeTabId = $state<string | null>(null);
let listenersReady = false;
let unlistenOutput: UnlistenFn | null = null;
let unlistenExit: UnlistenFn | null = null;
let unlistenSessionClosed: UnlistenFn | null = null;
let unlistenSessionDocked: UnlistenFn | null = null;
const resizeObservers = new Map<string, ResizeObserver>();

type ExitCallback = (sessionId: string) => void | Promise<void>;

let onSessionExit: ExitCallback | null = null;

function findTab(tabId: string): TerminalTab | undefined {
  return tabs.find((t) => t.id === tabId);
}

function findDetachedSession(sessionId: string): DetachedSession | undefined {
  return detachedSessions.find((s) => s.id === sessionId);
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

function attachTerminalIo(tabId: string, term: Terminal) {
  term.onData((data) => {
    invoke("write_pty", { sessionId: tabId, data }).catch(() => {});
  });
}

function openTerminalInstance(
  tabId: string,
  container: HTMLElement,
  options?: { banner?: string; replay?: string },
): Terminal {
  const term = new Terminal({
    cursorBlink: true,
    fontFamily: "JetBrains Mono, Courier New, monospace",
    fontSize: 13,
    lineHeight: 1,
    scrollback: 5000,
    theme: XTERM_THEME,
  });

  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(container);
  linkTerminal(tabId, term, fitAddon);
  attachResizeObserver(tabId, container, term, fitAddon);
  attachTerminalIo(tabId, term);

  if (options?.banner) {
    term.writeln(options.banner);
  }
  if (options?.replay) {
    term.write(options.replay);
  }

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

function removePopoutSession(sessionId: string) {
  popoutSessions = popoutSessions.filter((s) => s.id !== sessionId);
}

async function syncPopoutSessions() {
  try {
    const remote = await invoke<Array<{
      session_id: string;
      connection_name: string;
      window_label: string;
    }>>("list_popout_sessions");
    popoutSessions = remote.map((session) => ({
      id: session.session_id,
      name: session.connection_name,
      connectionName: session.connection_name,
      windowLabel: session.window_label,
    }));
  } catch {
    // Non-fatal if backend is unavailable during startup.
  }
}

async function mountReattachedSession(
  info: ReattachSessionPayload,
  banner: string,
): Promise<void> {
  if (findTab(info.session_id)) {
    activeTabId = info.session_id;
    return;
  }

  const tab: TerminalTab = {
    id: info.session_id,
    name: info.connection_name,
    connectionName: info.connection_name,
  };

  const term = await mountTerminalTab(tab, {
    banner,
    replay: info.buffered_output,
  });

  term?.scrollToBottom();
}

function removeDetachedSession(sessionId: string) {
  detachedSessions = detachedSessions.filter((s) => s.id !== sessionId);
}

function cleanupTabUi(tabId: string) {
  const tab = findTab(tabId);
  tab?.term?.dispose();
  detachResizeObserver(tabId);
  tabs = tabs.filter((t) => t.id !== tabId);

  if (activeTabId === tabId) {
    activeTabId = tabs.length > 0 ? tabs[0].id : null;
  }
}

/** Re-fit all terminals — e.g. when switching back to the Terminals tab. */
export function fitActiveTerminal() {
  for (const tab of tabs) {
    if (tab.term && tab.fitAddon) {
      fitTerminal(tab.id, tab.term, tab.fitAddon);
    }
  }
}

async function syncDetachedSessions() {
  try {
    const remote = await invoke<Array<{ session_id: string; connection_name: string }>>(
      "list_detached_sessions",
    );
    detachedSessions = remote.map((session) => ({
      id: session.session_id,
      name: session.connection_name,
      connectionName: session.connection_name,
    }));
  } catch {
    // Non-fatal if backend is unavailable during startup.
  }
}

async function syncActiveSessionCount() {
  try {
    activeSessionCount = await invoke<number>("count_active_sessions");
  } catch {
    activeSessionCount = tabs.length + detachedSessions.length;
  }
}

export async function initTerminalListeners(onExit?: ExitCallback) {
  if (listenersReady) return;
  listenersReady = true;
  onSessionExit = onExit ?? null;

  await syncDetachedSessions();
  await syncPopoutSessions();
  await syncActiveSessionCount();

  unlistenOutput = await listen("pty-output", (event) => {
    const payload = event.payload as { session_id?: string; sessionId?: string; data: string };
    const sessionId = payload.session_id ?? payload.sessionId;
    if (!sessionId) return;
    const term = findTab(sessionId)?.term;
    if (!term) return;
    const wasAtBottom = term.buffer.active.baseY + term.rows >= term.buffer.active.length;
    term.write(payload.data, () => {
      if (wasAtBottom) {
        term.scrollToBottom();
      }
    });
  });

  unlistenExit = await listen("pty-exit", (event) => {
    const sessionId = event.payload as string;

    if (findDetachedSession(sessionId)) {
      removeDetachedSession(sessionId);
      onSessionExit?.(sessionId);
      return;
    }

    if (popoutSessions.some((s) => s.id === sessionId)) {
      removePopoutSession(sessionId);
      onSessionExit?.(sessionId);
      return;
    }

    const tab = findTab(sessionId);
    tab?.term?.writeln("\n\x1b[1;33mSession disconnected.\x1b[0m");

    setTimeout(() => {
      cleanupTabUi(sessionId);
    }, 800);

    onSessionExit?.(sessionId);
  });

  unlistenSessionClosed = await listen("session-closed", async () => {
    await syncDetachedSessions();
    await syncPopoutSessions();
    await syncActiveSessionCount();
  });

  unlistenSessionDocked = await listen("session-docked", async (event) => {
    const info = event.payload as ReattachSessionPayload;
    removePopoutSession(info.session_id);
    await mountReattachedSession(
      info,
      `\x1b[1;36mDocked ${info.connection_name} to main window\x1b[0m`,
    );
    await syncActiveSessionCount();
  });
}

export async function teardownTerminalListeners() {
  unlistenOutput?.();
  unlistenExit?.();
  unlistenSessionClosed?.();
  unlistenSessionDocked?.();
  unlistenOutput = null;
  unlistenExit = null;
  unlistenSessionClosed = null;
  unlistenSessionDocked = null;
  listenersReady = false;
}

async function waitForTerminalContainer(tabId: string): Promise<HTMLElement | null> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const container = document.getElementById(`terminal-${tabId}`);
    if (container) return container;
    await tick();
  }
  return null;
}

async function mountTerminalTab(
  tab: TerminalTab,
  options?: { banner?: string; replay?: string },
): Promise<Terminal | null> {
  tabs = [...tabs, tab];
  activeTabId = tab.id;

  const container = await waitForTerminalContainer(tab.id);
  if (!container) {
    cleanupTabUi(tab.id);
    throw new Error("Terminal view is not ready. Try again.");
  }

  return openTerminalInstance(tab.id, container, options);
}

export async function connectSSH(conn: Connection): Promise<void> {
  const allowed = await ensureKerberosForConnection(conn);
  if (!allowed) return;

  const tabId = crypto.randomUUID().replace(/-/g, "").slice(0, 12);
  const newTab: TerminalTab = { id: tabId, name: conn.name, connectionName: conn.name };

  const term = await mountTerminalTab(newTab, {
    banner: `\x1b[1;36mInitializing Bayesian-SSH Shell Session to ${conn.name}... \x1b[0m`,
  });

  try {
    await invoke("spawn_pty", { sessionId: tabId, connectionName: conn.name });
    await syncActiveSessionCount();
  } catch (e: unknown) {
    term?.writeln(`\n\x1b[1;31mError spawning terminal process: ${String(e)}\x1b[0m`);
  }
}

export async function detachTab(tabId: string): Promise<void> {
  const tab = findTab(tabId);
  if (!tab) return;

  await invoke("detach_pty", { sessionId: tabId });

  cleanupTabUi(tabId);

  detachedSessions = [
    ...detachedSessions,
    {
      id: tab.id,
      name: tab.name,
      connectionName: tab.connectionName,
    },
  ];
  await syncActiveSessionCount();
}

export async function popOutTab(tabId: string): Promise<void> {
  const tab = findTab(tabId);
  if (!tab) return;

  await invoke("open_terminal_window", { sessionId: tabId, title: tab.name });
  cleanupTabUi(tabId);
  await syncPopoutSessions();
  await syncActiveSessionCount();
}

export async function popOutDetachedSession(sessionId: string): Promise<void> {
  const session = findDetachedSession(sessionId);
  if (!session) return;

  await invoke("open_terminal_window", { sessionId, title: session.name });
  removeDetachedSession(sessionId);
  await syncPopoutSessions();
  await syncActiveSessionCount();
}

export async function terminateAllDetachedSessions(): Promise<void> {
  for (const session of [...detachedSessions]) {
    await terminateDetachedSession(session.id);
  }
  await syncActiveSessionCount();
}

export async function reattachSession(sessionId: string): Promise<void> {
  if (findTab(sessionId)) {
    activeTabId = sessionId;
    return;
  }

  const detached = findDetachedSession(sessionId);
  if (!detached) {
    throw new Error("Detached session not found.");
  }

  const info = await invoke<ReattachSessionPayload>("reattach_pty", { sessionId });

  removeDetachedSession(sessionId);

  await mountReattachedSession(
    info,
    `\x1b[1;36mReattached to ${info.connection_name}\x1b[0m`,
  );
  await syncActiveSessionCount();
}

export async function dockPopoutSession(sessionId: string): Promise<void> {
  const popout = popoutSessions.find((s) => s.id === sessionId);
  if (!popout) {
    throw new Error("Pop-out session not found.");
  }

  await invoke("dock_popout_session", {
    sessionId,
    windowLabel: popout.windowLabel,
  });
}

export async function focusPopoutSession(sessionId: string): Promise<void> {
  await invoke("focus_terminal_window", { sessionId });
}

export async function terminatePopoutSession(sessionId: string): Promise<void> {
  try {
    await invoke("close_pty", { sessionId });
  } catch {
    // Session may already be closed.
  }
  removePopoutSession(sessionId);
  await syncActiveSessionCount();
}

export async function terminateDetachedSession(sessionId: string): Promise<void> {
  try {
    await invoke("close_pty", { sessionId });
  } catch {
    // Session may already be closed.
  }
  removeDetachedSession(sessionId);
  await syncActiveSessionCount();
}

export async function disconnectTab(tabId: string): Promise<void> {
  try {
    await invoke("close_pty", { sessionId: tabId });
  } catch {
    // Session may already be closed.
  }

  cleanupTabUi(tabId);
  removeDetachedSession(tabId);
  await syncActiveSessionCount();
}

export async function closeAllTabs(): Promise<number> {
  const count = await invoke<number>("close_all_ptys");
  for (const tab of [...tabs]) {
    tab.term?.dispose();
    detachResizeObserver(tab.id);
  }
  tabs = [];
  detachedSessions = [];
  popoutSessions = [];
  activeTabId = null;
  activeSessionCount = 0;
  return count;
}

export function getTerminalState() {
  return {
    get tabs() {
      return tabs;
    },
    get detachedSessions() {
      return detachedSessions;
    },
    get popoutSessions() {
      return popoutSessions;
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
    get detachedCount() {
      return detachedSessions.length;
    },
    get popoutCount() {
      return popoutSessions.length;
    },
    get externalSessionCount() {
      return detachedSessions.length + popoutSessions.length;
    },
    get totalSessionCount() {
      return activeSessionCount;
    },
  };
}
