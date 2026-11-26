import { tick } from "svelte";
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Terminal } from "@xterm/xterm";
import type { SearchAddon } from "@xterm/addon-search";
import type { Connection } from "$lib/types";
import { ensureKerberosForConnection } from "$lib/stores/kerberos.svelte";
import { notify } from "$lib/stores/notifications.svelte";
import {
  createXtermSession,
  safeWrite,
  syncPtySize,
  waitForTerminalFont,
  type PtyChunk,
} from "$lib/utils/terminal-xterm";
import { snapshotTerminal } from "$lib/utils/terminal-snapshot";
import {
  containsPane,
  insertPane,
  paneIds,
  removePane,
  setSplitRatio,
  type SplitNode,
  type SplitSide,
} from "$lib/utils/split-layout";
import {
  groupAtDrop,
  groupContiguous,
  nextTabGroupColor,
  type TabGroup,
} from "$lib/utils/tab-groups";
import {
  requestAttentionIfUnfocused,
  setWindowProgress,
  type ProgressState,
} from "$lib/utils/terminal-signals";
import { buildTerminalOptions } from "$lib/utils/terminal-options";
import {
  describeConnectFailure,
  parsePendingChangeError,
  stripTerminalControls,
  type ConnectFailure,
} from "$lib/utils/connect-failure";
import {
  createRuntime,
  disposeRuntime,
  flushRuntime,
  MAX_PENDING_OUTPUT_BYTES,
  remeasureFonts,
  runtimes,
  scheduleFit,
  setBlinkSuspended,
} from "$lib/stores/terminal-runtime";
import {
  getTerminalFontSize,
  initThemeSyncForTerminals,
  teardownThemeSyncForTerminals,
  resetTerminalFontSize,
  updateTerminalFontSize,
} from "$lib/stores/terminal-appearance.svelte";
import {
  addGroup,
  deleteGroup,
  expandTabGroup,
  findGroup,
  getTabGroups,
  groupForHost,
  rememberHost,
  setGroupCollapsed,
} from "$lib/stores/tab-groups.svelte";

export type TerminalTabStatus = "connecting" | "connected" | "exited" | "error";

export interface PendingChangePrompt {
  tabId: string;
  connection: Connection;
  changes: string[];
  detectedAt: string;
}

/** Route change waiting for the user; renders `ConnectionChangeModal`. */
let pendingChange = $state<PendingChangePrompt | null>(null);

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
  /** Tab group this tab belongs to, if any. */
  readonly groupId: string | null;
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
  groupId = $state<string | null>(null);

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

/** Metadata only; replaced wholesale on add/remove. */
let tabs = $state.raw<TabState[]>([]);
let detachedSessions = $state<DetachedSession[]>([]);
let popoutSessions = $state<PopoutSession[]>([]);
let activeSessionCount = $state(0);
let activeTabId = $state<string | null>(null);
/** Split layouts; each shows two or more tabs side by side. */
let splits = $state.raw<SplitNode[]>([]);
/** Tabs whose terminal is on screen: the active tab's split, or the active tab alone. */
let shownIds: string[] = [];
/** Group of a session while it runs in the background or in its own window. */
const awayGroups = new Map<string, string>();
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

function splitOf(tabId: string | null): SplitNode | undefined {
  return tabId ? splits.find((root) => containsPane(root, tabId)) : undefined;
}

/** Show the terminals now on screen and release the renderers of the rest. */
function updateShownTerminals() {
  const root = splitOf(activeTabId);
  const next = root ? paneIds(root) : activeTabId ? [activeTabId] : [];
  for (const id of shownIds) {
    if (!next.includes(id)) runtimes.get(id)?.session?.setVisible(false);
  }
  shownIds = next;
  if (terminalsVisible) revealShownTerminals();
}

function revealShownTerminals() {
  for (const id of shownIds) {
    const rt = runtimes.get(id);
    if (!rt) continue;
    rt.session?.setVisible(true);
    flushRuntime(rt);
    scheduleFit(id);
  }
}

export function flushPendingTabOutput(tab: Pick<TerminalTab, "id">) {
  const rt = runtimes.get(tab.id);
  if (rt) flushRuntime(rt);
}

export function setActiveTab(tabId: string | null) {
  if (activeTabId === tabId) return;

  activeTabId = tabId;
  updateShownTerminals();
  if (!tabId) {
    void setWindowProgress(0, 0);
    return;
  }

  const tab = findTab(tabId);
  if (tab) {
    tab.bell = false;
    if (tab.groupId) expandTabGroup(tab.groupId);
    if (tab.progress) {
      void setWindowProgress(tab.progress.state, tab.progress.value);
    } else {
      void setWindowProgress(0, 0);
    }
  }

  const rt = runtimes.get(tabId);
  if (rt?.term) {
    requestAnimationFrame(() => rt.term?.focus());
  }
}

/** False while another view covers the Terminals panel. */
let terminalsVisible = false;

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
  if (visible) revealShownTerminals();
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
      updateTerminalFontSize(getTerminalFontSize() + delta);
    },
    signals: {
      onTitle: (title) => {
        if (tab) {
          tab.remoteTitle = title ? title.trim() : null;
        }
      },
      onBell: () => {
        const isTabActive = shownIds.includes(tabId) && terminalsVisible;
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

  const isVisible = shownIds.includes(tabId) && terminalsVisible;
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
  const groupId = awayGroups.get(info.session_id);
  awayGroups.delete(info.session_id);
  if (findGroup(groupId ?? null)) tab.groupId = groupId!;
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

/** Take a tab out of its split; a split left with one pane dissolves. */
function leaveSplit(tabId: string) {
  splits = splits.flatMap((root) => {
    if (!containsPane(root, tabId)) return [root];
    const rest = removePane(root, tabId);
    return rest?.kind === "split" ? [rest] : [];
  });
}

function cleanupTabUi(tabId: string) {
  disposeRuntime(tabId);
  const root = splitOf(tabId);
  const panes = root ? paneIds(root) : [];
  const at = panes.indexOf(tabId);
  const paneNeighbour = panes[at + 1] ?? panes[at - 1];
  leaveSplit(tabId);
  shownIds = shownIds.filter((id) => id !== tabId);
  if (findTab(tabId)) tabs = tabs.filter((t) => t.id !== tabId);

  if (activeTabId === tabId) {
    setActiveTab(paneNeighbour ?? (tabs.length > 0 ? tabs[0].id : null));
  } else {
    updateShownTerminals();
  }
}

/** Re-fit the active terminal — e.g. when switching back to the Terminals
 *  view. Hidden tabs are fitted when they become active. */
export function fitActiveTerminal() {
  for (const id of shownIds) scheduleFit(id);
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
    try {
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
          awayGroups.delete(sessionId);
          removeDetachedSession(sessionId);
          return;
        }

        if (popoutSessions.some((s) => s.id === sessionId)) {
          awayGroups.delete(sessionId);
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
    } catch (err) {
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
      throw err;
    } finally {
      listenersInFlight = null;
    }
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
  teardownThemeSyncForTerminals();
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

/** New tabs go after the last tab of their group, or at the end. */
function withTabPlaced(tab: TabState): TabState[] {
  let at = tabs.length;
  if (tab.groupId) {
    tabs.forEach((t, i) => {
      if (t.groupId === tab.groupId) at = i + 1;
    });
  }
  return [...tabs.slice(0, at), tab, ...tabs.slice(at)];
}

async function mountTerminalTab(
  tab: TabState,
  options?: { banner?: string; replay?: string; split?: { targetId: string; side: SplitSide } },
): Promise<Terminal | null> {
  // The runtime exists before the xterm instance so output arriving during
  // creation is buffered rather than dropped.
  createRuntime(tab.id);
  tabs = withTabPlaced(tab);
  if (options?.split && findTab(options.split.targetId)) {
    placeInSplit(tab.id, options.split.targetId, options.split.side);
  }
  setActiveTab(tab.id);

  const container = await waitForTerminalContainer(tab.id);
  if (!container) {
    cleanupTabUi(tab.id);
    throw new Error("Terminal view is not ready. Try again.");
  }

  return openTerminalInstance(tab.id, container, options);
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

export interface ConnectOptions {
  /** Always start a new session, even when one to this host is open. */
  newTab?: boolean;
  /** Open the new session beside the active tab, on this side. Implies `newTab`. */
  split?: SplitSide;
}

/**
 * Connect to a host. Without `newTab`, a tab already open (or connecting) to
 * the same host is brought forward instead of starting a second session.
 */
export async function connectSSH(conn: Connection, options: ConnectOptions = {}): Promise<void> {
  if (!options.newTab && !options.split) {
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
  // A host remembered by a group opens in it.
  newTab.groupId = groupForHost(conn.id);
  if (newTab.groupId) expandTabGroup(newTab.groupId);

  const target = options.split ? activeTabId : null;
  const term = await mountTerminalTab(
    newTab,
    target && options.split ? { split: { targetId: target, side: options.split } } : undefined,
  );
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

/** Next/previous tab, skipping tabs hidden in a collapsed group. */
function cycleTab(step: 1 | -1) {
  const reachable = tabs.filter((t) => t.id === activeTabId || !findGroup(t.groupId)?.collapsed);
  if (reachable.length < 2) return;
  const index = reachable.findIndex((t) => t.id === activeTabId);
  const next = reachable[(index + step + reachable.length) % reachable.length];
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
      updateTerminalFontSize(getTerminalFontSize() + 1);
      return true;
    case "term-zoom-out":
      updateTerminalFontSize(getTerminalFontSize() - 1);
      return true;
    case "term-zoom-reset":
      resetTerminalFontSize();
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

  if (tab.groupId) awayGroups.set(tab.id, tab.groupId);
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
  if (tab.groupId) awayGroups.set(tab.id, tab.groupId);
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
  awayGroups.delete(sessionId);
  removePopoutSession(sessionId);
  await syncActiveSessionCount();
}

export async function terminateDetachedSession(sessionId: string): Promise<void> {
  try {
    await invoke("close_pty", { sessionId });
  } catch {
    // Session may already be closed.
  }
  awayGroups.delete(sessionId);
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
  splits = [];
  shownIds = [];
  awayGroups.clear();
  detachedSessions = [];
  popoutSessions = [];
  activeTabId = null;
  activeSessionCount = 0;
  return count;
}

// ---- Split screen ------------------------------------------------------

function placeInSplit(tabId: string, targetTabId: string, side: SplitSide) {
  leaveSplit(tabId);
  const root = splitOf(targetTabId);
  const next = insertPane(root ?? { kind: "pane", tabId: targetTabId }, targetTabId, tabId, side);
  splits = root ? splits.map((r) => (r === root ? next : r)) : [...splits, next];
}

/** Show `tabId` beside `targetTabId`, on `side` of it, and focus it. */
export function splitTab(tabId: string, targetTabId: string, side: SplitSide) {
  if (tabId === targetTabId || !findTab(tabId) || !findTab(targetTabId)) return;
  placeInSplit(tabId, targetTabId, side);
  if (activeTabId !== tabId) setActiveTab(tabId);
  else updateShownTerminals();
}

/** Take a tab out of its split; it keeps running as a normal tab. */
export function unsplitTab(tabId: string) {
  if (!splitOf(tabId)) return;
  leaveSplit(tabId);
  updateShownTerminals();
}

export function resizeSplit(splitId: string, ratio: number) {
  splits = splits.map((root) => setSplitRatio(root, splitId, ratio));
}

// ---- Tab groups --------------------------------------------------------




function assignGroup(tab: TabState, groupId: string | null) {
  const previous = tab.groupId;
  if (previous === groupId) return;
  tab.groupId = groupId;
  const connId = tab.conn?.id;
  if (!connId) return;
  if (groupId) {
    rememberHost(connId, groupId);
  } else if (!tabs.some((t) => t !== tab && t.groupId === previous && t.conn?.id === connId)) {
    rememberHost(connId, null);
  }
}

/** Set a tab's group and move it right after the tabs of `anchorGroupId`. */
function regroupTab(tab: TabState, groupId: string | null, anchorGroupId: string) {
  assignGroup(tab, groupId);
  const rest = tabs.filter((t) => t !== tab);
  let at = tabs.indexOf(tab);
  rest.forEach((t, i) => {
    if (t.groupId === anchorGroupId) at = i + 1;
  });
  tabs = groupContiguous([...rest.slice(0, at), tab, ...rest.slice(at)]);
}

/** Start a group with this tab. Returns the new group's id. */
export function createTabGroup(tabId: string, name = ""): string | undefined {
  if (!findTab(tabId)) return undefined;
  const group: TabGroup = {
    id: crypto.randomUUID(),
    name: name.trim(),
    color: nextTabGroupColor(getTabGroups()),
    collapsed: false,
    hosts: [],
  };
  addGroup(group);
  addTabToGroup(tabId, group.id);
  return group.id;
}

export function addTabToGroup(tabId: string, groupId: string) {
  const tab = findTab(tabId);
  if (!tab || !findGroup(groupId) || tab.groupId === groupId) return;
  regroupTab(tab, groupId, groupId);
  expandTabGroup(groupId);
}

export function removeTabFromGroup(tabId: string) {
  const tab = findTab(tabId);
  if (!tab?.groupId) return;
  regroupTab(tab, null, tab.groupId);
}

/**
 * Move a tab before `beforeTabId` (to the end when null). Dropped inside a
 * group it joins it; dropped outside its own group it leaves it.
 */
export function moveTab(tabId: string, beforeTabId: string | null) {
  const tab = findTab(tabId);
  if (!tab || tabId === beforeTabId) return;
  const rest = tabs.filter((t) => t !== tab);
  let at = beforeTabId ? rest.findIndex((t) => t.id === beforeTabId) : rest.length;
  if (at < 0) at = rest.length;
  assignGroup(tab, groupAtDrop(tab.groupId, rest[at - 1], rest[at]));
  tabs = groupContiguous([...rest.slice(0, at), tab, ...rest.slice(at)]);
}

/** Collapsing the active tab's group moves to the nearest tab still in view. */
export function toggleTabGroupCollapsed(groupId: string) {
  const group = findGroup(groupId);
  if (!group) return;
  setGroupCollapsed(groupId, !group.collapsed);
  if (group.collapsed) return;
  const active = findTab(activeTabId ?? "");
  if (active?.groupId !== groupId) return;
  const at = tabs.indexOf(active);
  const inView = (t: TabState) => !findGroup(t.groupId)?.collapsed;
  const next = tabs.slice(at + 1).find(inView) ?? tabs.slice(0, at).reverse().find(inView);
  if (next) setActiveTab(next.id);
}

/** Dissolve a group: its tabs stay open, the group and its hosts are forgotten. */
export function ungroupTabGroup(groupId: string) {
  for (const tab of tabs) {
    if (tab.groupId === groupId) tab.groupId = null;
  }
  for (const [sessionId, id] of awayGroups) {
    if (id === groupId) awayGroups.delete(sessionId);
  }
  deleteGroup(groupId);
}

/** Close every session of a group. The group keeps its hosts for later. */
export async function closeTabGroup(groupId: string): Promise<void> {
  for (const tab of tabs.filter((t) => t.groupId === groupId)) {
    await disconnectTab(tab.id);
  }
}

/** Connect to every host a group remembers, each in a new tab of the group. */
export async function openTabGroup(groupId: string, connections: readonly Connection[]): Promise<void> {
  const group = findGroup(groupId);
  if (!group) return;
  const hosts = connections.filter((c) => group.hosts.includes(c.id));
  if (hosts.length === 0) {
    notify(`No saved host is left in "${group.name || "this group"}"`, "info");
    return;
  }
  for (const conn of hosts) {
    await connectSSH(conn, { newTab: true });
  }
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
    /** Tab groups, open or remembered, in creation order. */
    get groups(): readonly TabGroup[] {
      return getTabGroups();
    },
    /** Split holding the active tab, when it is shown beside others. */
    get activeSplit(): SplitNode | undefined {
      return splitOf(activeTabId);
    },
    /** Every split layout, including those of tabs not in view. */
    get splits(): readonly SplitNode[] {
      return splits;
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
