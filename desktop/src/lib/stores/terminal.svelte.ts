import { tick } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { SearchAddon } from "@xterm/addon-search";
import type { Connection, DesktopSettings } from "$lib/types";
import { ensureKerberosForConnection } from "$lib/stores/kerberos.svelte";
import { notify } from "$lib/stores/notifications.svelte";
import { getCurrentXtermTheme } from "$lib/utils/theme";
import {
  attachXtermAddons,
  attachXtermIo,
  attachXtermKeyHandler,
  attachXtermLinuxInputFix,
  attachXtermMouseHandlers,
  createOutputCoalescer,
  safeWrite,
  type OutputCoalescer,
} from "$lib/utils/terminal-xterm";

type SettingsGetter = () => DesktopSettings | undefined;
let _settingsGetter: SettingsGetter | null = null;

export function registerSettingsGetter(getter: SettingsGetter) {
  _settingsGetter = getter;
}

export function getTerminalSettings(): DesktopSettings | undefined {
  return _settingsGetter?.();
}

export function buildTerminalOptions(settings?: DesktopSettings): Record<string, unknown> {
  const s = settings ?? _settingsGetter?.();
  return {
    cursorBlink: s?.terminal_cursor_blink ?? true,
    cursorStyle: (s?.terminal_cursor_style ?? "block") as "block" | "bar" | "underline" | undefined,
    fontFamily:
      s?.terminal_font_family ||
      "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace",
    fontSize: s?.terminal_font_size ?? 13,
    lineHeight: s?.terminal_line_height ?? 1.18,
    scrollback: s?.terminal_scrollback ?? 10000,
    smoothScrollDuration: 120,
    fontLigatures: true,
    fontWeight: "normal",
    theme: getCurrentXtermTheme(),
    allowProposedApi: true,
  };
}

export interface TerminalTab {
  id: string;
  name: string;
  connectionName: string;
  term?: Terminal;
  fitAddon?: FitAddon;
  searchAddon?: SearchAddon;
  compositionGuardCleanup?: () => void;
  mouseCleanup?: () => void;
  showSearch?: boolean;
  outputCoalescer?: OutputCoalescer;
  /** Output that arrived while the tab existed but the xterm instance was
   *  still being created (reattach flow) — flushed once the terminal opens. */
  pendingOutput?: string[];
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

let terminalFontSize = $state(13);
let themeSyncInitialized = false;

export function initThemeSyncForTerminals() {
  if (themeSyncInitialized || typeof window === "undefined") return;
  themeSyncInitialized = true;

  const observer = new MutationObserver(() => {
    applyThemeToAllTerminals();
  });
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
}
export function applyThemeToAllTerminals(settings?: DesktopSettings) {
  const currentTheme = getCurrentXtermTheme();
  tabs.forEach((tab) => {
    if (tab.term) {
      tab.term.options.theme = currentTheme;
      if (settings?.terminal_font_family) {
        tab.term.options.fontFamily = settings.terminal_font_family;
      }
      if (settings?.terminal_font_size) {
        tab.term.options.fontSize = settings.terminal_font_size;
      }
      if (settings?.terminal_line_height) {
        tab.term.options.lineHeight = settings.terminal_line_height;
      }
      if (settings?.terminal_cursor_style) {
        tab.term.options.cursorStyle = settings.terminal_cursor_style;
      }
      if (settings?.terminal_cursor_blink !== undefined) {
        tab.term.options.cursorBlink = settings.terminal_cursor_blink;
      }
      if (settings?.terminal_scrollback) {
        tab.term.options.scrollback = settings.terminal_scrollback;
      }
    }
  });
  fitActiveTerminal();
}

export function getTerminalFontSize(): number {
  return terminalFontSize;
}

export function updateTerminalFontSize(newSize: number) {
  terminalFontSize = Math.max(8, Math.min(32, newSize));
  const settings = getTerminalSettings();
  if (settings?.terminal_font_size !== terminalFontSize) {
    settings!.terminal_font_size = terminalFontSize;
  }
  tabs.forEach((tab) => {
    if (tab.term && tab.fitAddon) {
      tab.term.options.fontSize = terminalFontSize;
      fitTerminal(tab.id, tab.term, tab.fitAddon);
    }
  });
}

let tabs = $state<TerminalTab[]>([]);
let detachedSessions = $state<DetachedSession[]>([]);
let popoutSessions = $state<PopoutSession[]>([]);
let activeSessionCount = $state(0);
let activeTabId = $state<string | null>(null);
let listenersReady = false;
let listenersInFlight: Promise<void> | null = null;
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

/** Route one PTY output event to its tab, coalesced. */
const MAX_PENDING_OUTPUT_BYTES = 256 * 1024;

function deliverPtyOutput(sessionId: string, data: string) {
  if (!data) return;
  const tab = findTab(sessionId);
  if (!tab) return;

  if (tab.term && tab.outputCoalescer) {
    tab.outputCoalescer.push(data);
    return;
  }
  if (!tab.pendingOutput) tab.pendingOutput = [];
  if (getPendingOutputBytes(tab.pendingOutput) + data.length > MAX_PENDING_OUTPUT_BYTES) {
    console.warn("pendingOutput queue exceeded limit, dropping oldest chunks");
    while (tab.pendingOutput.length > 0 && getPendingOutputBytes(tab.pendingOutput) + data.length > MAX_PENDING_OUTPUT_BYTES) {
      tab.pendingOutput.shift();
    }
  }
  tab.pendingOutput.push(data);
}

function getPendingOutputBytes(chunks: string[]): number {
  return chunks.reduce((sum, c) => sum + c.length, 0);
}

function linkTerminal(
  tabId: string,
  term: Terminal,
  fitAddon: FitAddon,
  searchAddon?: SearchAddon,
  compositionGuardCleanup?: () => void,
  mouseCleanup?: () => void,
  outputCoalescer?: OutputCoalescer,
) {
  const index = tabs.findIndex((t) => t.id === tabId);
  if (index !== -1) {
    tabs[index].term = term;
    tabs[index].fitAddon = fitAddon;
    tabs[index].searchAddon = searchAddon;
    tabs[index].compositionGuardCleanup = compositionGuardCleanup;
    tabs[index].mouseCleanup = mouseCleanup;
    tabs[index].outputCoalescer = outputCoalescer;
    tabs[index].showSearch = false;
  }
}

export function toggleTerminalSearch(tabId?: string) {
  const id = tabId ?? activeTabId;
  if (!id) return;
  const index = tabs.findIndex((t) => t.id === id);
  if (index !== -1) {
    tabs[index].showSearch = !tabs[index].showSearch;
    tabs = [...tabs];
  }
}

export function closeTerminalSearch(tabId?: string) {
  const id = tabId ?? activeTabId;
  if (!id) return;
  const index = tabs.findIndex((t) => t.id === id);
  if (index !== -1) {
    tabs[index].showSearch = false;
    tabs = [...tabs];
  }
}

const fitRequests = new Map<string, number>();

function isContainerVisible(container: HTMLElement | null | undefined): boolean {
  if (!container) return false;
  // display:none (hidden tab) or detached from the document → skip fitting.
  return container.isConnected && container.offsetParent !== null;
}

function fitTerminal(tabId: string, term: Terminal, fitAddon: FitAddon) {
  const container = term.element?.parentElement;
  if (!isContainerVisible(container)) return;

  // Throttle to one fit + resize IPC per animation frame; ResizeObserver
  // fires continuously while the window is being resized.
  const pending = fitRequests.get(tabId);
  if (pending) {
    cancelAnimationFrame(pending);
  }

  const raf = requestAnimationFrame(() => {
    fitRequests.delete(tabId);
    try {
      fitAddon.fit();
      if (term.cols === 0 || term.rows === 0) return;
      invoke("resize_pty", {
        sessionId: tabId,
        cols: term.cols,
        rows: term.rows,
      }).catch(() => {});
    } catch {
      // Container may not have dimensions yet.
    }
  });
  fitRequests.set(tabId, raf);
}

function attachTerminalIo(tabId: string, term: Terminal) {
  attachXtermIo(tabId, term);
}

function openTerminalInstance(
  tabId: string,
  container: HTMLElement,
  options?: { banner?: string; replay?: string },
): Terminal {
  const term = new Terminal(buildTerminalOptions() as ConstructorParameters<typeof Terminal>[0]);

  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);

  const { searchAddon } = attachXtermAddons(term);

  term.open(container);
  const compositionGuardCleanup = attachXtermLinuxInputFix(container, term);
  const mouseCleanup = attachXtermMouseHandlers(container, term);
  const outputCoalescer = createOutputCoalescer(term);

  linkTerminal(tabId, term, fitAddon, searchAddon, compositionGuardCleanup, mouseCleanup, outputCoalescer);
  attachResizeObserver(tabId, container, term, fitAddon);
  attachTerminalIo(tabId, term);

  container.addEventListener("mousedown", () => term.focus());

  attachXtermKeyHandler(term, () => {
    toggleTerminalSearch(tabId);
  });

  // Ctrl / Cmd + Mouse Wheel zoom event listener
  container.addEventListener("wheel", (e) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      if (e.deltaY < 0) {
        updateTerminalFontSize(terminalFontSize + 1);
      } else {
        updateTerminalFontSize(terminalFontSize - 1);
      }
    }
  }, { passive: false });

  // Order matters: replay (older buffered history) first, then any live
  // output that arrived while this terminal was being created (reattach
  // race), then the banner.
  if (options?.replay) {
    safeWrite(term, options.replay, () => {
      term.scrollToBottom();
    });
  }

  const pending = findTab(tabId)?.pendingOutput;
  if (pending && pending.length > 0) {
    findTab(tabId)!.pendingOutput = undefined;
    for (const chunk of pending) {
      outputCoalescer.push(chunk);
    }
  }

  if (options?.banner && !options?.replay) {
    safeWrite(term, `${options.banner}\r\n`, () => {
      term.scrollToBottom();
    });
  }

  requestAnimationFrame(() => term.focus());

  return term;
}

async function sealSessionUi(sessionId: string): Promise<void> {
  try {
    await invoke("seal_session_ui", { sessionId });
  } catch {
    // Non-fatal if the backend session is already gone.
  }
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

async function mountReattachedSession(info: ReattachSessionPayload): Promise<void> {
  if (findTab(info.session_id)) {
    activeTabId = info.session_id;
    return;
  }

  const tab: TerminalTab = {
    id: info.session_id,
    name: info.connection_name,
    connectionName: info.connection_name,
  };

  tabs = [...tabs, tab];
  activeTabId = tab.id;

  const container = await waitForTerminalContainer(tab.id);
  if (!container) {
    cleanupTabUi(tab.id);
    throw new Error("Terminal view is not ready. Try again.");
  }

  const term = openTerminalInstance(tab.id, container, {
    replay: info.buffered_output,
  });

  await tick();
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      const mounted = findTab(tab.id);
      if (mounted?.term && mounted.fitAddon) {
        fitTerminal(tab.id, mounted.term, mounted.fitAddon);
        mounted.term.scrollToBottom();
      }
    });
  });
}

function removeDetachedSession(sessionId: string) {
  detachedSessions = detachedSessions.filter((s) => s.id !== sessionId);
}

function cleanupTabUi(tabId: string) {
  const tab = findTab(tabId);
  tab?.outputCoalescer?.dispose();
  tab?.compositionGuardCleanup?.();
  tab?.mouseCleanup?.();
  tab?.term?.dispose();
  detachResizeObserver(tabId);
  const pendingFit = fitRequests.get(tabId);
  if (pendingFit) {
    cancelAnimationFrame(pendingFit);
    fitRequests.delete(tabId);
  }
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

/** Focus the active terminal tab's xterm instance. */
export function focusActiveTerminal() {
  if (!activeTabId) return;
  const tab = findTab(activeTabId);
  if (!tab?.term) return;
  tab.term.focus();
  if (tab.fitAddon) {
    fitTerminal(activeTabId, tab.term, tab.fitAddon);
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
  if (listenersInFlight) {
    // Registration already in progress — wait for it to finish.
    await listenersInFlight;
    if (listenersReady) return;
  }

  onSessionExit = onExit ?? null;

  const registration = (async () => {
    initThemeSyncForTerminals();

    await syncDetachedSessions();
    await syncPopoutSessions();
    await syncActiveSessionCount();

    unlistenOutput = await listen("pty-output", (event) => {
      const payload = event.payload as { session_id?: string; sessionId?: string; data: string };
      const sessionId = payload.session_id ?? payload.sessionId;
      if (!sessionId) return;
      deliverPtyOutput(sessionId, payload.data);
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
      // Flush pending output first so the disconnect notice lands after it.
      tab?.outputCoalescer?.flush();
      safeWrite(tab?.term, "\n\x1b[1;33mSession disconnected.\x1b[0m\r\n");

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
      await mountReattachedSession(info);
      notify(`"${info.connection_name}" docked — output restored`, "success");
      await syncActiveSessionCount();
    });

    listenersReady = true;
    listenersInFlight = null;
  })();

  listenersInFlight = registration;
  await registration;
}

export async function teardownTerminalListeners() {
  // Wait for an in-flight registration to settle before tearing down, so
  // listeners registered after the teardown can't leak.
  if (listenersInFlight) {
    await listenersInFlight.catch(() => {});
  }
  unlistenOutput?.();
  unlistenExit?.();
  unlistenSessionClosed?.();
  unlistenSessionDocked?.();
  unlistenOutput = null;
  unlistenExit = null;
  unlistenSessionClosed = null;
  unlistenSessionDocked = null;
  listenersReady = false;
  listenersInFlight = null;
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
    const message = String(e);
    safeWrite(term ?? undefined, `\n\x1b[1;31mFailed to start session: ${message}\x1b[0m\r\n`);
    // Keep the tab open so the user can read the error; auto-close it after
    // a few seconds unless they interact with it.
    setTimeout(() => {
      if (findTab(tabId)?.id === tabId) {
        cleanupTabUi(tabId);
      }
    }, 8000);
  }
}

export async function detachTab(tabId: string): Promise<void> {
  const tab = findTab(tabId);
  if (!tab) return;

  await sealSessionUi(tabId);
  try {
    await invoke("detach_pty", { sessionId: tabId });
  } catch (e: unknown) {
    // Session is already gone — don't leave a ghost tab behind.
    cleanupTabUi(tabId);
    notify(`Failed to detach "${tab.name}": ${String(e)}`, "error");
    return;
  }

  cleanupTabUi(tabId);

  detachedSessions = [
    ...detachedSessions,
    {
      id: tab.id,
      name: tab.name,
      connectionName: tab.connectionName,
    },
  ];
  notify(`"${tab.name}" is running in the background — your program is still active`, "info");
  await syncActiveSessionCount();
}

export async function popOutTab(tabId: string): Promise<void> {
  const tab = findTab(tabId);
  if (!tab) return;

  await sealSessionUi(tabId);
  try {
    await invoke("open_terminal_window", { sessionId: tabId, title: tab.name });
  } catch (e: unknown) {
    // Window could not be created — keep the tab so the session stays usable.
    notify(`Failed to open terminal window: ${String(e)}`, "error");
    return;
  }
  cleanupTabUi(tabId);
  notify(`"${tab.name}" opened in a separate window — session continues`, "info");
  await syncPopoutSessions();
  await syncActiveSessionCount();
}

export async function popOutDetachedSession(sessionId: string): Promise<void> {
  const session = findDetachedSession(sessionId);
  if (!session) return;

  try {
    await invoke("open_terminal_window", { sessionId, title: session.name });
  } catch (e: unknown) {
    notify(`Failed to open terminal window: ${String(e)}`, "error");
    return;
  }
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

  await mountReattachedSession(info);
  notify(`"${info.connection_name}" reattached — output restored`, "success");
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
    tab.outputCoalescer?.dispose();
    tab.compositionGuardCleanup?.();
    tab.mouseCleanup?.();
    tab.term?.dispose();
    detachResizeObserver(tab.id);
  }
  for (const raf of fitRequests.values()) {
    cancelAnimationFrame(raf);
  }
  fitRequests.clear();
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
      if (value) {
        requestAnimationFrame(() => focusActiveTerminal());
      }
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
