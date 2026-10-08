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
  attachXtermPtyResize,
  createOutputCoalescer,
  safeWrite,
  syncPtySize,
  waitForTerminalFont,
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

/** Reactive tab metadata. The xterm runtime lives outside reactivity. */
export interface TerminalTab {
  readonly id: string;
  readonly name: string;
  readonly connectionName: string;
  /** In-terminal search bar open. */
  showSearch: boolean;
  status: TerminalTabStatus;
  /** xterm instance mounted and usable. */
  ready: boolean;
  /** Live xterm instance (non-reactive lookup; check `ready` in templates). */
  readonly term: Terminal | undefined;
  readonly searchAddon: SearchAddon | undefined;
}

/** Non-reactive per-tab runtime: xterm objects, buffers and cleanups. */
interface TabRuntime {
  term?: Terminal;
  fitAddon?: FitAddon;
  searchAddon?: SearchAddon;
  coalescer?: OutputCoalescer;
  /** Output received while the tab is hidden or still being created. */
  pending: string[];
  pendingBytes: number;
  cleanups: Array<() => void>;
  fitRaf: number | null;
  /** Cursor blink turned off while the Terminals view is hidden. */
  blinkSuspended: boolean;
}

const runtimes = new Map<string, TabRuntime>();

class TabState implements TerminalTab {
  readonly id: string;
  readonly name: string;
  readonly connectionName: string;
  showSearch = $state(false);
  status = $state<TerminalTabStatus>("connecting");
  ready = $state(false);

  constructor(id: string, name: string, connectionName: string, status: TerminalTabStatus) {
    this.id = id;
    this.name = name;
    this.connectionName = connectionName;
    this.status = status;
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
let unlistenOutput: UnlistenFn | null = null;
let unlistenExit: UnlistenFn | null = null;
let unlistenSessionClosed: UnlistenFn | null = null;
let unlistenSessionDocked: UnlistenFn | null = null;

type ExitCallback = (sessionId: string) => void | Promise<void>;

let onSessionExit: ExitCallback | null = null;

function findTab(tabId: string): TabState | undefined {
  return tabs.find((t) => t.id === tabId);
}

function findDetachedSession(sessionId: string): DetachedSession | undefined {
  return detachedSessions.find((s) => s.id === sessionId);
}

/** Cap on output buffered for a hidden tab before it is handed to xterm. */
const MAX_PENDING_OUTPUT_BYTES = 256 * 1024;

function flushRuntime(rt: TabRuntime) {
  if (!rt.term || rt.pending.length === 0) return;
  const combined = rt.pending.length === 1 ? rt.pending[0] : rt.pending.join("");
  rt.pending = [];
  rt.pendingBytes = 0;
  if (rt.coalescer) {
    rt.coalescer.push(combined);
  } else {
    safeWrite(rt.term, combined);
  }
}

export function flushPendingTabOutput(tab: Pick<TerminalTab, "id">) {
  const rt = runtimes.get(tab.id);
  if (rt) flushRuntime(rt);
}

export function setActiveTab(tabId: string | null) {
  if (activeTabId === tabId) return;
  activeTabId = tabId;
  if (!tabId) return;
  const rt = runtimes.get(tabId);
  if (!rt) return;
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
  for (const rt of runtimes.values()) setBlinkSuspended(rt, !visible);
  if (!visible || !activeTabId) return;
  const rt = runtimes.get(activeTabId);
  if (rt) flushRuntime(rt);
}

function deliverPtyOutput(sessionId: string, data: string) {
  if (!data) return;
  // Cheap ownership guard: sessions shown elsewhere (pop-out windows,
  // detached) have no runtime in this window.
  const rt = runtimes.get(sessionId);
  if (!rt) return;

  if (terminalsVisible && sessionId === activeTabId && rt.coalescer) {
    // Anything buffered while the tab was hidden must land before new output.
    if (rt.pending.length > 0) flushRuntime(rt);
    rt.coalescer.push(data);
    return;
  }

  // Background tab (or terminal still being created): buffer in memory
  // without triggering xterm redraws / layout computations.
  rt.pending.push(data);
  rt.pendingBytes += data.length;
  if (rt.pendingBytes <= MAX_PENDING_OUTPUT_BYTES) return;
  if (rt.coalescer) {
    // Hand a busy hidden tab's output to xterm instead of silently dropping
    // scrollback (and splitting escape sequences) once the buffer is full.
    flushRuntime(rt);
    return;
  }
  while (rt.pending.length > 1 && rt.pendingBytes > MAX_PENDING_OUTPUT_BYTES) {
    rt.pendingBytes -= rt.pending.shift()!.length;
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
  const rt: TabRuntime = {
    pending: [],
    pendingBytes: 0,
    cleanups: [],
    fitRaf: null,
    blinkSuspended: false,
  };
  runtimes.set(tabId, rt);
  return rt;
}

function disposeRuntime(tabId: string) {
  const rt = runtimes.get(tabId);
  if (!rt) return;
  runtimes.delete(tabId);
  if (rt.fitRaf !== null) cancelAnimationFrame(rt.fitRaf);
  rt.coalescer?.dispose();
  for (const cleanup of rt.cleanups) {
    try {
      cleanup();
    } catch {
      // Best effort.
    }
  }
  rt.term?.dispose();
  rt.pending = [];
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

  const term = new Terminal(termOptions as ConstructorParameters<typeof Terminal>[0]);
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  const { searchAddon } = attachXtermAddons(term);

  term.open(container);
  const outputCoalescer = createOutputCoalescer(term);

  rt.term = term;
  rt.fitAddon = fitAddon;
  rt.searchAddon = searchAddon;
  rt.coalescer = outputCoalescer;
  rt.cleanups.push(attachXtermLinuxInputFix(container, term));
  rt.cleanups.push(attachXtermMouseHandlers(container, term));

  attachXtermIo(tabId, term);
  attachXtermPtyResize(tabId, term);

  const observer = new ResizeObserver(() => scheduleFit(tabId));
  observer.observe(container);
  rt.cleanups.push(() => observer.disconnect());

  const onMouseDown = () => term.focus();
  const onWheel = (e: WheelEvent) => {
    // Ctrl / Cmd + wheel zoom.
    if (!(e.ctrlKey || e.metaKey)) return;
    e.preventDefault();
    updateTerminalFontSize(terminalFontSize + (e.deltaY < 0 ? 1 : -1));
  };
  container.addEventListener("mousedown", onMouseDown);
  container.addEventListener("wheel", onWheel, { passive: false });
  rt.cleanups.push(() => {
    container.removeEventListener("mousedown", onMouseDown);
    container.removeEventListener("wheel", onWheel);
  });

  attachXtermKeyHandler(term, () => {
    toggleTerminalSearch(tabId);
  });

  if (!terminalsVisible) setBlinkSuspended(rt, true);

  // Order matters: replay (older buffered history) first, then any live
  // output that arrived while this terminal was being created (reattach
  // race), then the banner.
  if (options?.replay) {
    safeWrite(term, options.replay, () => {
      term.scrollToBottom();
    });
  }

  if (rt.pending.length > 0) flushRuntime(rt);

  if (options?.banner && !options?.replay) {
    safeWrite(term, `${options.banner}\r\n`, () => {
      term.scrollToBottom();
    });
  }

  const tab = findTab(tabId);
  if (tab) tab.ready = true;

  requestAnimationFrame(() => {
    scheduleFit(tabId);
    term.focus();
  });

  return term;
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
      if (tab) tab.status = "exited";
      const rt = runtimes.get(sessionId);
      if (rt) {
        // Flush pending output first so the disconnect notice lands after it.
        flushRuntime(rt);
        rt.coalescer?.flush();
        safeWrite(rt.term, "\n\x1b[1;33mSession disconnected.\x1b[0m\r\n");
      }

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
      try {
        await mountReattachedSession(info);
        notify(`"${info.connection_name}" docked — output restored`, "success");
      } catch (e: unknown) {
        notify(`Failed to dock "${info.connection_name}": ${String(e)}`, "error");
      }
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

export async function connectSSH(conn: Connection): Promise<void> {
  const allowed = await ensureKerberosForConnection(conn);
  if (!allowed) return;

  const tabId = crypto.randomUUID().replace(/-/g, "").slice(0, 12);
  const newTab = new TabState(tabId, conn.name, conn.name, "connecting");

  const term = await mountTerminalTab(newTab, {
    banner: `\x1b[1;36mInitializing Bayesian-SSH Shell Session to ${conn.name}... \x1b[0m`,
  });
  // Closed before the terminal finished mounting.
  if (!term) return;

  try {
    await invoke("spawn_pty", { sessionId: tabId, connectionName: conn.name });
    if (newTab.status === "connecting") newTab.status = "connected";
    // Resizes during spawn were sent before the PTY existed.
    const live = runtimes.get(tabId)?.term;
    if (live) syncPtySize(tabId, live);
    await syncActiveSessionCount();
  } catch (e: unknown) {
    newTab.status = "error";
    const message = String(e);
    safeWrite(term, `\n\x1b[1;31mFailed to start session: ${message}\x1b[0m\r\n`);
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
    setActiveTab(sessionId);
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
