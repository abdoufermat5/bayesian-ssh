import { tick } from "svelte";
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { SearchAddon } from "@xterm/addon-search";
import type { Connection, DesktopSettings } from "$lib/types";
import { ensureKerberosForConnection } from "$lib/stores/kerberos.svelte";
import { notify } from "$lib/stores/notifications.svelte";
import { getCurrentXtermTheme } from "$lib/utils/theme";
import {
  createXtermSession,
  safeWrite,
  syncPtySize,
  waitForTerminalFont,
  type PtyChunk,
  type XtermSessionHandle,
} from "$lib/utils/terminal-xterm";
import { snapshotTerminal } from "$lib/utils/terminal-snapshot";
import {
  requestAttentionIfUnfocused,
  setWindowProgress,
  type ProgressState,
} from "$lib/utils/terminal-signals";
type SettingsGetter = () => DesktopSettings | undefined;
let _settingsGetter: SettingsGetter | null = null;

export function registerSettingsGetter(getter: SettingsGetter) {
  _settingsGetter = getter;
}

export function getTerminalSettings(): DesktopSettings | undefined {
  return _settingsGetter?.();
}

export const DEFAULT_TERMINAL_FONT_FAMILY =
  "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace";

export function buildTerminalOptions(settings?: DesktopSettings): Record<string, unknown> {
  const s = settings ?? _settingsGetter?.();
  return {
    cursorBlink: s?.terminal_cursor_blink ?? true,
    cursorStyle: (s?.terminal_cursor_style ?? "block") as "block" | "bar" | "underline" | undefined,
    fontFamily: s?.terminal_font_family || DEFAULT_TERMINAL_FONT_FAMILY,
    fontSize: s?.terminal_font_size ?? 13,
    lineHeight: s?.terminal_line_height ?? 1.18,
    scrollback: s?.terminal_scrollback ?? 10000,
    // Smooth scrolling animates every wheel tick and costs frames under load.
    smoothScrollDuration: 0,
    fontWeight: "normal",
    theme: getCurrentXtermTheme(),
    allowProposedApi: true,
  };
}

export type TerminalTabStatus = "connecting" | "connected" | "exited" | "error";

/** Why a connection attempt ended, in plain words, plus ssh's own last lines. */
export interface ConnectFailure {
  title: string;
  hint: string;
  detail: string;
}

/** Prefix spawn_pty uses for a team-sync route change waiting for confirmation. */
const PENDING_CHANGE_PREFIX = "connection-change-pending:";

/** A team-sync route change the desktop must show before connecting. */
interface PendingChangeError {
  name: string;
  detected_at: string;
  changes: string[];
}

export interface PendingChangePrompt {
  tabId: string;
  connection: Connection;
  changes: string[];
  detectedAt: string;
}

/** Route change waiting for the user; renders `ConnectionChangeModal`. */
let pendingChange = $state<PendingChangePrompt | null>(null);

function parsePendingChangeError(e: unknown): PendingChangeError | null {
  const message = String(e);
  if (!message.startsWith(PENDING_CHANGE_PREFIX)) return null;
  try {
    return JSON.parse(message.slice(PENDING_CHANGE_PREFIX.length)) as PendingChangeError;
  } catch {
    return null;
  }
}

/** ssh exits with 255 for its own errors; one that ends this soon never connected. */
const SSH_ERROR_EXIT = 255;
const CONNECT_WINDOW_MS = 30_000;

/** Reactive tab metadata. The xterm runtime lives outside reactivity. */
export interface TerminalTab {
  readonly id: string;
  readonly name: string;
  readonly connectionName: string;
  /** In-terminal search bar open. */
  showSearch: boolean;
  status: TerminalTabStatus;
  /** Host this tab connects to; absent for re-attached sessions. */
  readonly conn: Connection | undefined;
  /** When the current connection attempt started (ms since epoch). */
  connectStartedAt: number;
  /** Set when status is "error". */
  failure: ConnectFailure | null;
  /** xterm instance mounted and usable. */
  ready: boolean;
  /** Live xterm instance (non-reactive lookup; check `ready` in templates). */
  readonly term: Terminal | undefined;
  readonly searchAddon: SearchAddon | undefined;
  remoteTitle: string | null;
  bell: boolean;
  progress: { state: ProgressState; value: number } | null;
}

/** Non-reactive per-tab runtime: xterm objects, buffers and cleanups. */
interface TabRuntime {
  session?: XtermSessionHandle;
  term?: Terminal;
  fitAddon?: FitAddon;
  searchAddon?: SearchAddon;
  /** Output received while the tab is hidden or still being created. */
  pending: Array<{ data: string; bytes: number }>;
  pendingBytes: number;
  fitRaf: number | null;
  /** Cursor blink turned off while the Terminals view is hidden. */
  blinkSuspended: boolean;
  /** Plain-text tail of ssh's output, kept to explain a failed connect. */
  recentOutput: string;
}

const runtimes = new Map<string, TabRuntime>();

class TabState implements TerminalTab {
  readonly id: string;
  readonly name: string;
  readonly connectionName: string;
  showSearch = $state(false);
  status = $state<TerminalTabStatus>("connecting");
  ready = $state(false);
  readonly conn: Connection | undefined;
  connectStartedAt = $state(Date.now());
  failure = $state.raw<ConnectFailure | null>(null);
  remoteTitle = $state<string | null>(null);
  bell = $state(false);
  progress = $state<{ state: ProgressState; value: number } | null>(null);

  constructor(id: string, name: string, connectionName: string, status: TerminalTabStatus, conn?: Connection) {
    this.id = id;
    this.name = name;
    this.connectionName = connectionName;
    this.status = status;
    this.conn = conn;
  }

  get term(): Terminal | undefined {
    return runtimes.get(this.id)?.term;
  }

  get searchAddon(): SearchAddon | undefined {
    return runtimes.get(this.id)?.searchAddon;
  }
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
/** Size from Settings; zoom changes `terminalFontSize` and, below, the settings object. */
let configuredFontSize: number | null = null;
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
  if (settings?.terminal_font_size) {
    // Keep keyboard/wheel zoom relative to the saved size, not the default.
    terminalFontSize = settings.terminal_font_size;
    configuredFontSize = settings.terminal_font_size;
  }
  for (const rt of runtimes.values()) {
    const term = rt.term;
    if (!term) continue;
    term.options.theme = currentTheme;
    if (settings?.terminal_font_family) term.options.fontFamily = settings.terminal_font_family;
    if (settings?.terminal_font_size) term.options.fontSize = settings.terminal_font_size;
    if (settings?.terminal_line_height) term.options.lineHeight = settings.terminal_line_height;
    if (settings?.terminal_cursor_style) term.options.cursorStyle = settings.terminal_cursor_style;
    // While hidden, blink stays off; the saved value is restored on show.
    if (settings?.terminal_cursor_blink !== undefined && !rt.blinkSuspended) {
      term.options.cursorBlink = settings.terminal_cursor_blink;
    }
    if (settings?.terminal_scrollback) term.options.scrollback = settings.terminal_scrollback;
  }
  fitActiveTerminal();
}

export function getTerminalFontSize(): number {
  return terminalFontSize;
}

export function updateTerminalFontSize(newSize: number) {
  terminalFontSize = Math.max(8, Math.min(32, newSize));
  const settings = getTerminalSettings();
  configuredFontSize ??= settings?.terminal_font_size ?? 13;
  if (settings && settings.terminal_font_size !== terminalFontSize) {
    settings.terminal_font_size = terminalFontSize;
  }
  for (const [id, rt] of runtimes) {
    if (!rt.term) continue;
    rt.term.options.fontSize = terminalFontSize;
    // Hidden tabs are skipped by scheduleFit and refit when activated.
    scheduleFit(id);
  }
}

/** Metadata only; replaced wholesale on add/remove. */
let tabs = $state.raw<TabState[]>([]);
let detachedSessions = $state<DetachedSession[]>([]);
let popoutSessions = $state<PopoutSession[]>([]);
let activeSessionCount = $state(0);
let activeTabId = $state<string | null>(null);
let listenersReady = false;
let listenersInFlight: Promise<void> | null = null;
let unlistenExit: UnlistenFn | null = null;
let unlistenExitStatus: UnlistenFn | null = null;
/** Exit code of a finished session; arrives just before its `pty-exit`. */
const exitCodes = new Map<string, number>();
let unlistenSessionClosed: UnlistenFn | null = null;
let unlistenSessionDocked: UnlistenFn | null = null;
let unlistenHistoryUpdated: UnlistenFn | null = null;

type HistoryCallback = () => void | Promise<void>;

/**
 * Called on `history-updated`, which the backend emits only after a session's
 * end has been written to the database. Reloading on `pty-exit` instead raced
 * that write and left closed sessions displayed as "Running".
 */
let onHistoryUpdated: HistoryCallback | null = null;

function findTab(tabId: string): TabState | undefined {
  return tabs.find((t) => t.id === tabId);
}

function findDetachedSession(sessionId: string): DetachedSession | undefined {
  return detachedSessions.find((s) => s.id === sessionId);
}

/** Cap on output buffered for a hidden tab before it is handed to xterm. */
const MAX_PENDING_OUTPUT_BYTES = 256 * 1024;

function flushRuntime(rt: TabRuntime) {
  if (!rt.session || rt.pending.length === 0) return;
  const chunks = rt.pending;
  rt.pending = [];
  rt.pendingBytes = 0;
  for (const chunk of chunks) {
    rt.session.write(chunk.data, chunk.bytes);
  }
}

export function flushPendingTabOutput(tab: Pick<TerminalTab, "id">) {
  const rt = runtimes.get(tab.id);
  if (rt) flushRuntime(rt);
}

export function setActiveTab(tabId: string | null) {
  if (activeTabId === tabId) return;

  if (activeTabId) {
    const oldRt = runtimes.get(activeTabId);
    oldRt?.session?.setVisible(false);
  }

  activeTabId = tabId;
  if (!tabId) {
    void setWindowProgress(0, 0);
    return;
  }

  const tab = findTab(tabId);
  if (tab) {
    tab.bell = false;
    if (tab.progress) {
      void setWindowProgress(tab.progress.state, tab.progress.value);
    } else {
      void setWindowProgress(0, 0);
    }
  }

  const rt = runtimes.get(tabId);
  if (!rt) return;

  if (terminalsVisible) {
    rt.session?.setVisible(true);
  }

  flushRuntime(rt);
  if (rt.term) {
    requestAnimationFrame(() => {
      scheduleFit(tabId);
      rt.term?.focus();
    });
  }
}

/** False while another view covers the Terminals panel. */
let terminalsVisible = false;

function desiredCursorBlink(): boolean {
  return getTerminalSettings()?.terminal_cursor_blink ?? true;
}

function setBlinkSuspended(rt: TabRuntime, suspend: boolean) {
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

/** Pause rendering output into xterm (and cursor blink) while the Terminals
 *  view is hidden; buffered output is flushed into the active tab on show. */
export function setTerminalsVisible(visible: boolean) {
  if (terminalsVisible === visible) return;
  terminalsVisible = visible;
  for (const rt of runtimes.values()) {
    setBlinkSuspended(rt, !visible);
    if (!visible) {
      rt.session?.setVisible(false);
    }
  }
  if (!visible || !activeTabId) return;
  const rt = runtimes.get(activeTabId);
  if (rt) {
    rt.session?.setVisible(true);
    flushRuntime(rt);
    scheduleFit(activeTabId);
  }
}

function deliverPtyOutput(sessionId: string, data: string, bytes: number) {
  if (!data && bytes <= 0) return;
  const rt = runtimes.get(sessionId);
  if (!rt) {
    if (bytes > 0) {
      invoke("ack_pty", { sessionId, bytes }).catch(() => {});
    }
    return;
  }

  // ssh's first output (banner, password prompt, remote prompt) means it is
  // past connecting; its text also explains a failure.
  const tab = findTab(sessionId);
  if (tab?.status === "connecting") tab.status = "connected";
  if (tab && Date.now() - tab.connectStartedAt < CONNECT_WINDOW_MS) {
    rt.recentOutput = (rt.recentOutput + stripTerminalControls(data)).slice(-4000);
  }

  if (rt.session) {
    if (rt.pending.length > 0) flushRuntime(rt);
    rt.session.write(data, bytes);
    return;
  }

  // Terminal still being created: buffer in memory until session mounts.
  rt.pending.push({ data, bytes });
  rt.pendingBytes += bytes;

  while (rt.pending.length > 0 && rt.pendingBytes > MAX_PENDING_OUTPUT_BYTES) {
    const dropped = rt.pending.shift()!;
    rt.pendingBytes -= dropped.bytes;
    if (dropped.bytes > 0) {
      invoke("ack_pty", { sessionId, bytes: dropped.bytes }).catch(() => {});
    }
  }
}

export function toggleTerminalSearch(tabId?: string) {
  const id = tabId ?? activeTabId;
  if (!id) return;
  const tab = findTab(id);
  if (tab) tab.showSearch = !tab.showSearch;
}

export function closeTerminalSearch(tabId?: string) {
  const id = tabId ?? activeTabId;
  if (!id) return;
  const tab = findTab(id);
  if (tab) tab.showSearch = false;
}

function isContainerVisible(container: HTMLElement | null | undefined): boolean {
  if (!container) return false;
  // display:none (hidden tab/view) or detached from the document → skip.
  return container.isConnected && container.offsetParent !== null;
}

/**
 * Fit a tab's terminal at most once per animation frame. `fit()` only
 * resizes when cols/rows change, and the PTY is told via `term.onResize`,
 * so unchanged layouts never reach the backend.
 */
function scheduleFit(tabId: string) {
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

function createRuntime(tabId: string): TabRuntime {
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

function disposeRuntime(tabId: string) {
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

/** Re-measure open terminals when the font finished loading after timeout. */
function remeasureFonts() {
  for (const [id, rt] of runtimes) {
    const term = rt.term;
    if (!term) continue;
    const family = term.options.fontFamily;
    term.options.fontFamily = "monospace";
    term.options.fontFamily = family;
    scheduleFit(id);
  }
}

async function openTerminalInstance(
  tabId: string,
  container: HTMLElement,
  options?: { banner?: string; replay?: string },
): Promise<Terminal | null> {
  const termOptions = buildTerminalOptions();
  await waitForTerminalFont(String(termOptions.fontFamily), remeasureFonts);

  const rt = runtimes.get(tabId);
  // Tab closed while the font was loading.
  if (!rt) return null;
  if (!container.isConnected) {
    cleanupTabUi(tabId);
    return null;
  }

  const tab = findTab(tabId);

  const session = createXtermSession(container, tabId, {
    terminalOptions: termOptions,
    getConnectionName: () => tab?.name ?? "terminal",
    onOpenSearch: () => {
      toggleTerminalSearch(tabId);
    },
    onShortcut: runTerminalShortcut,
    onZoom: (delta) => {
      updateTerminalFontSize(terminalFontSize + delta);
    },
    signals: {
      onTitle: (title) => {
        if (tab) {
          tab.remoteTitle = title ? title.trim() : null;
        }
      },
      onBell: () => {
        const isTabActive = tabId === activeTabId && terminalsVisible;
        getCurrentWindow().isFocused().then((isFocused) => {
          if (tab && (!isTabActive || !isFocused)) {
            tab.bell = true;
          }
        }).catch(() => {
          if (tab && (!isTabActive || !document.hasFocus())) {
            tab.bell = true;
          }
        });
        void requestAttentionIfUnfocused();
      },
      onProgress: (state, value) => {
        if (tab) {
          tab.progress = state === 0 ? null : { state, value };
          if (tabId === activeTabId) {
            void setWindowProgress(state, value);
          }
        }
      },
    },
  });

  rt.session = session;
  rt.term = session.term;
  rt.fitAddon = session.fitAddon;
  rt.searchAddon = session.searchAddon;

  const isVisible = tabId === activeTabId && terminalsVisible;
  session.setVisible(isVisible);

  if (!terminalsVisible) setBlinkSuspended(rt, true);

  // Order matters: replay (older buffered history) first, then any live
  // output that arrived while this terminal was being created (reattach
  // race), then the banner.
  if (options?.replay) {
    safeWrite(session.term, options.replay, () => {
      session.term.scrollToBottom();
      invoke("redraw_pty", { sessionId: tabId }).catch(() => {});
    });
  }

  if (rt.pending.length > 0) flushRuntime(rt);

  if (options?.banner && !options?.replay) {
    safeWrite(session.term, `${options.banner}\r\n`, () => {
      session.term.scrollToBottom();
    });
  }

  if (tab) tab.ready = true;

  requestAnimationFrame(() => {
    scheduleFit(tabId);
    session.term.focus();
  });

  return session.term;
}

async function sealSessionUi(sessionId: string): Promise<void> {
  try {
    await invoke("seal_session_ui", { sessionId });
  } catch {
    // Non-fatal if the backend session is already gone.
  }
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
    setActiveTab(info.session_id);
    return;
  }

  const tab = new TabState(info.session_id, info.connection_name, info.connection_name, "connected");
  const term = await mountTerminalTab(tab, { replay: info.buffered_output });
  if (!term) return;

  // The PTY kept the size of its previous view; align it with this one once
  // the new terminal has been fitted.
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      syncPtySize(tab.id, term);
      term.scrollToBottom();
      invoke("redraw_pty", { sessionId: tab.id }).catch(() => {});
    });
  });
}

function removeDetachedSession(sessionId: string) {
  detachedSessions = detachedSessions.filter((s) => s.id !== sessionId);
}

function cleanupTabUi(tabId: string) {
  disposeRuntime(tabId);
  if (findTab(tabId)) tabs = tabs.filter((t) => t.id !== tabId);

  if (activeTabId === tabId) {
    setActiveTab(tabs.length > 0 ? tabs[0].id : null);
  }
}

/** Re-fit the active terminal — e.g. when switching back to the Terminals
 *  view. Hidden tabs are fitted when they become active. */
export function fitActiveTerminal() {
  if (activeTabId) scheduleFit(activeTabId);
}

/** Focus the active terminal tab's xterm instance. */
export function focusActiveTerminal() {
  if (!activeTabId) return;
  const term = runtimes.get(activeTabId)?.term;
  if (!term) return;
  term.focus();
  scheduleFit(activeTabId);
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

export async function initTerminalListeners(onHistoryChanged?: HistoryCallback) {
  if (listenersReady) return;
  if (listenersInFlight) {
    // Registration already in progress — wait for it to finish.
    await listenersInFlight;
    if (listenersReady) return;
  }

  onHistoryUpdated = onHistoryChanged ?? null;

  const registration = (async () => {
    initThemeSyncForTerminals();

    await syncDetachedSessions();
    await syncPopoutSessions();
    await syncActiveSessionCount();

    unlistenExitStatus = await listen("pty-exit-status", (event) => {
      const payload = event.payload as { session_id: string; exit_code: number };
      exitCodes.set(payload.session_id, payload.exit_code);
    });

    unlistenExit = await listen("pty-exit", (event) => {
      const sessionId = event.payload as string;
      const exitCode = exitCodes.get(sessionId);
      exitCodes.delete(sessionId);

      if (findDetachedSession(sessionId)) {
        removeDetachedSession(sessionId);
        return;
      }

      if (popoutSessions.some((s) => s.id === sessionId)) {
        removePopoutSession(sessionId);
        return;
      }

      const tab = findTab(sessionId);
      const rt = runtimes.get(sessionId);
      if (
        tab &&
        exitCode === SSH_ERROR_EXIT &&
        tab.status !== "exited" &&
        Date.now() - tab.connectStartedAt < CONNECT_WINDOW_MS
      ) {
        // ssh gave up while connecting: keep the tab and explain, don't close it.
        if (rt) {
          flushRuntime(rt);
        }
        failConnect(tab, describeConnectFailure(rt?.recentOutput ?? ""));
        void syncActiveSessionCount();
        return;
      }
      if (tab) {
        tab.status = "exited";
        tab.progress = null;
        if (tab.id === activeTabId) {
          void setWindowProgress(0, 0);
        }
      }
      if (rt) {
        // Flush pending output first so the disconnect notice lands after it.
        flushRuntime(rt);
        safeWrite(rt.term, "\n\x1b[1;33mSession disconnected.\x1b[0m\r\n");
      }

      setTimeout(() => {
        cleanupTabUi(sessionId);
      }, 800);
    });

    unlistenSessionClosed = await listen("session-closed", async () => {
      await syncDetachedSessions();
      await syncPopoutSessions();
      await syncActiveSessionCount();
    });

    unlistenSessionDocked = await listen("session-docked", async (event) => {
      const payload = event.payload as { session_id?: string; sessionId?: string; connection_name?: string; connectionName?: string };
      const sessionId = payload.session_id ?? payload.sessionId;
      if (!sessionId) return;
      const connectionName = payload.connection_name ?? payload.connectionName ?? "Terminal";
      removePopoutSession(sessionId);
      createRuntime(sessionId);
      try {
        const outputChannel = new Channel<PtyChunk>((chunk) => {
          deliverPtyOutput(sessionId, chunk.data, chunk.bytes);
        });
        const info = await invoke<ReattachSessionPayload>("attach_pty_output", {
          sessionId,
          output: outputChannel,
        });
        await mountReattachedSession(info);
        notify(`"${info.connection_name || connectionName}" docked — output restored`, "success");
      } catch (e: unknown) {
        disposeRuntime(sessionId);
        notify(`Failed to dock "${connectionName}": ${String(e)}`, "error");
      }
      await syncActiveSessionCount();
    });

    unlistenHistoryUpdated = await listen("history-updated", () => {
      void onHistoryUpdated?.();
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
  unlistenExit?.();
  unlistenExitStatus?.();
  unlistenSessionClosed?.();
  unlistenSessionDocked?.();
  unlistenHistoryUpdated?.();
  unlistenExit = null;
  unlistenExitStatus = null;
  unlistenSessionClosed = null;
  unlistenSessionDocked = null;
  unlistenHistoryUpdated = null;
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
  tab: TabState,
  options?: { banner?: string; replay?: string },
): Promise<Terminal | null> {
  // The runtime exists before the xterm instance so output arriving during
  // creation is buffered rather than dropped.
  createRuntime(tab.id);
  tabs = [...tabs, tab];
  activeTabId = tab.id;

  const container = await waitForTerminalContainer(tab.id);
  if (!container) {
    cleanupTabUi(tab.id);
    throw new Error("Terminal view is not ready. Try again.");
  }

  return openTerminalInstance(tab.id, container, options);
}

const ANSI_PATTERN = new RegExp(
  String.raw`\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]`,
  "g",
);

function stripTerminalControls(data: string): string {
  return data.replace(ANSI_PATTERN, "").replace(/\r/g, "");
}

/** Plain-words explanation of the last ssh output; the raw lines stay as detail. */
function describeConnectFailure(output: string): ConnectFailure {
  const detail = output
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(-6)
    .join("\n");
  const has = (re: RegExp) => re.test(detail);
  if (has(/REMOTE HOST IDENTIFICATION HAS CHANGED/i)) {
    return {
      title: "The host's identity changed",
      hint: "Its host key no longer matches the one saved earlier. Don't connect until you know why: it can mean a reinstall, or someone in the middle.",
      detail,
    };
  }
  if (has(/host key verification failed/i)) {
    return {
      title: "The host key wasn't accepted",
      hint: "ssh couldn't verify this host's key. Check it with the host's owner, then add it to known_hosts.",
      detail,
    };
  }
  if (has(/permission denied|too many authentication failures|no supported authentication/i)) {
    return {
      title: "The host rejected your credentials",
      hint: "Check the user name and the key or password. Edit the host to change them.",
      detail,
    };
  }
  if (has(/could not resolve hostname|name or service not known|nodename nor servname/i)) {
    return {
      title: "That host name can't be found",
      hint: "Check the address for typos, and that your network or VPN is up.",
      detail,
    };
  }
  if (has(/connection refused/i)) {
    return {
      title: "The host refused the connection",
      hint: "Nothing accepted the connection on that port. Check the port, and that the SSH service is running.",
      detail,
    };
  }
  if (has(/timed out|no route to host|network is unreachable/i)) {
    return {
      title: "The host didn't answer",
      hint: "It may be offline, firewalled, or only reachable through a VPN or bastion.",
      detail,
    };
  }
  if (has(/connection closed|connection reset|broken pipe|kex_exchange_identification/i)) {
    return {
      title: "The host closed the connection",
      hint: "It hung up before login finished. A firewall, a connection limit or a bastion setting may be the cause.",
      detail,
    };
  }
  return {
    title: "ssh couldn't connect",
    hint: detail ? "ssh reported:" : "ssh stopped without saying why.",
    detail,
  };
}

function failConnect(tab: TabState, failure: ConnectFailure) {
  tab.failure = failure;
  tab.status = "error";
}

/** Closed-tab history for "Reopen closed tab", newest last. */
const closedTabs: Connection[] = [];
const MAX_CLOSED_TABS = 10;

async function startSession(tab: TabState, conn: Connection): Promise<void> {
  tab.failure = null;
  tab.status = "connecting";
  tab.connectStartedAt = Date.now();
  const rt = createRuntime(tab.id);
  rt.recentOutput = "";
  try {
    const outputChannel = new Channel<PtyChunk>((chunk) => {
      deliverPtyOutput(tab.id, chunk.data, chunk.bytes);
    });
    await invoke("spawn_pty", {
      sessionId: tab.id,
      connectionName: conn.name,
      output: outputChannel,
    });
    if (!findTab(tab.id)) {
      // Cancelled or closed while spawning.
      await invoke("close_pty", { sessionId: tab.id }).catch(() => {});
      return;
    }
    // The tab stays "connecting" until ssh prints something (see deliverPtyOutput):
    // spawn_pty only means the process started, not that the host answered.
    // Resizes during spawn were sent before the PTY existed.
    const live = runtimes.get(tab.id)?.term;
    if (live) syncPtySize(tab.id, live);
    await syncActiveSessionCount();
  } catch (e: unknown) {
    if (!findTab(tab.id)) return;
    // A team sync moved this host. Never connect until the user has seen
    // the old and the new route and accepted it.
    const pending = parsePendingChangeError(e);
    if (pending) {
      pendingChange = {
        tabId: tab.id,
        connection: conn,
        changes: pending.changes,
        detectedAt: pending.detected_at,
      };
      failConnect(tab, {
        title: `"${conn.name}" was changed by a team sync`,
        hint: "Confirm the new host, bastion and credentials before connecting.",
        detail: pending.changes.join("\n"),
      });
      return;
    }
    const message = String(e);
    failConnect(tab, {
      title: `Couldn't start the session to ${conn.name}`,
      hint: "The app couldn't start ssh for this host:",
      detail: message,
    });
  }
}

/** Accept the pending team-sync change and retry the blocked connection. */
export async function acceptPendingChange(): Promise<void> {
  const prompt = pendingChange;
  if (!prompt) return;
  pendingChange = null;
  try {
    await invoke("confirm_connection_change", { idOrName: prompt.connection.name });
  } catch (e: unknown) {
    notify(`Failed to confirm the change: ${String(e)}`, "error");
    return;
  }
  const tab = findTab(prompt.tabId);
  if (tab) await startSession(tab, prompt.connection);
}

/** Keep the change unconfirmed: the tab stays in its error state. */
export function dismissPendingChange(): void {
  pendingChange = null;
}

/**
 * Connect to a host. Without `newTab`, a tab already open (or connecting) to
 * the same host is brought forward instead of starting a second session.
 */
export async function connectSSH(conn: Connection, options: { newTab?: boolean } = {}): Promise<void> {
  if (!options.newTab) {
    const open = tabs.find((t) => t.conn?.id === conn.id && (t.status === "connected" || t.status === "connecting"));
    if (open) {
      setActiveTab(open.id);
      requestAnimationFrame(() => focusActiveTerminal());
      return;
    }
  }

  const allowed = await ensureKerberosForConnection(conn);
  if (!allowed) return;

  const tabId = crypto.randomUUID().replace(/-/g, "").slice(0, 12);
  const newTab = new TabState(tabId, conn.name, conn.name, "connecting", conn);

  const term = await mountTerminalTab(newTab);
  // Closed before the terminal finished mounting.
  if (!term) return;

  await startSession(newTab, conn);
}

/** Try the same host again in the same tab, after a failed connect. */
export async function retryConnect(tabId: string): Promise<void> {
  const tab = findTab(tabId);
  if (!tab?.conn || tab.status !== "error") return;
  const allowed = await ensureKerberosForConnection(tab.conn);
  if (!allowed) return;
  runtimes.get(tabId)?.term?.reset();
  await startSession(tab, tab.conn);
  runtimes.get(tabId)?.term?.focus();
}

/** Abort a connection that is still in progress and close its tab. */
export async function cancelConnect(tabId: string): Promise<void> {
  const tab = findTab(tabId);
  if (!tab || tab.status !== "connecting") return;
  cleanupTabUi(tabId);
  try {
    await invoke("close_pty", { sessionId: tabId });
  } catch {
    // Not spawned yet; startSession closes it when spawn_pty returns.
  }
  await syncActiveSessionCount();
}

/** Reopen the most recently closed tab's host in a new tab. */
export async function reopenClosedTab(): Promise<void> {
  const conn = closedTabs.pop();
  if (!conn) {
    notify("No closed tab to reopen", "info");
    return;
  }
  await connectSSH(conn, { newTab: true });
}

function cycleTab(step: 1 | -1) {
  if (tabs.length < 2) return;
  const index = tabs.findIndex((t) => t.id === activeTabId);
  const next = tabs[(index + step + tabs.length) % tabs.length];
  setActiveTab(next.id);
}

/**
 * Run a TERMINAL_SHORTCUTS action. Returns false when it does not apply, so
 * the key reaches the shell (Esc with nothing connecting).
 */
export function runTerminalShortcut(id: string): boolean {
  switch (id) {
    case "term-next-tab":
      cycleTab(1);
      return true;
    case "term-prev-tab":
      cycleTab(-1);
      return true;
    case "term-close-tab":
      if (activeTabId) void disconnectTab(activeTabId);
      return true;
    case "term-reopen-tab":
      void reopenClosedTab();
      return true;
    case "term-zoom-in":
      updateTerminalFontSize(terminalFontSize + 1);
      return true;
    case "term-zoom-out":
      updateTerminalFontSize(terminalFontSize - 1);
      return true;
    case "term-zoom-reset":
      updateTerminalFontSize(configuredFontSize ?? 13);
      return true;
    case "term-cancel": {
      const tab = activeTabId ? findTab(activeTabId) : undefined;
      if (tab?.status !== "connecting") return false;
      void cancelConnect(tab.id);
      return true;
    }
    default:
      return false;
  }
}

export async function detachTab(tabId: string): Promise<void> {
  const tab = findTab(tabId);
  if (!tab) return;

  const rt = runtimes.get(tabId);
  const snapshot = rt?.term ? snapshotTerminal(rt.term) : null;
  await sealSessionUi(tabId);
  try {
    await invoke("detach_pty", { sessionId: tabId, snapshot });
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

  const rt = runtimes.get(tabId);
  const snapshot = rt?.term ? snapshotTerminal(rt.term) : null;
  await sealSessionUi(tabId);
  try {
    await invoke("open_terminal_window", { sessionId: tabId, title: tab.name, snapshot });
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
    await invoke("open_terminal_window", { sessionId, title: session.name, snapshot: null });
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
    setActiveTab(sessionId);
    return;
  }

  const detached = findDetachedSession(sessionId);
  if (!detached) {
    throw new Error("Detached session not found.");
  }
  createRuntime(sessionId);
  try {
    const outputChannel = new Channel<PtyChunk>((chunk) => {
      deliverPtyOutput(sessionId, chunk.data, chunk.bytes);
    });
    const info = await invoke<ReattachSessionPayload>("reattach_pty", {
      sessionId,
      output: outputChannel,
    });
    removeDetachedSession(sessionId);
    await mountReattachedSession(info);
    notify(`"${info.connection_name}" reattached — output restored`, "success");
    await syncActiveSessionCount();
  } catch (e: unknown) {
    disposeRuntime(sessionId);
    throw e;
  }
}

export async function dockPopoutSession(sessionId: string): Promise<void> {
  const popout = popoutSessions.find((s) => s.id === sessionId);
  if (!popout) {
    throw new Error("Pop-out session not found.");
  }

  await invoke("dock_popout_session", {
    sessionId,
    windowLabel: popout.windowLabel,
    snapshot: null,
  });
}

export async function focusPopoutSession(sessionId: string): Promise<void> {
  try {
    await invoke("focus_terminal_window", { sessionId });
  } catch (e: unknown) {
    notify(`Failed to focus terminal window: ${String(e)}`, "error");
  }
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
  const conn = findTab(tabId)?.conn;
  if (conn) {
    closedTabs.push(conn);
    if (closedTabs.length > MAX_CLOSED_TABS) closedTabs.shift();
  }
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
  for (const id of [...runtimes.keys()]) disposeRuntime(id);
  tabs = [];
  detachedSessions = [];
  popoutSessions = [];
  activeTabId = null;
  activeSessionCount = 0;
  return count;
}

export function getTerminalState() {
  return {
    get tabs(): readonly TerminalTab[] {
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
    /** Metadata of the active tab, if any. */
    get activeTab(): TerminalTab | undefined {
      return activeTabId ? findTab(activeTabId) : undefined;
    },
    set activeTabId(value: string | null) {
      setActiveTab(value);
    },
    get count() {
      return tabs.length;
    },
    get pendingChange() {
      return pendingChange;
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
