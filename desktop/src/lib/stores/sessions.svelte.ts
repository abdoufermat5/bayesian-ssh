/**
 * Session / terminal registry glue and app navigation state.
 *
 * Owns the active tab, sidebar collapsed flag, session history entries and
 * the terminal-facing glue used by components: connecting a host, switching
 * tabs, reattaching/docking/terminating sessions, and quit/close-all flows.
 * All xterm interaction is delegated to `terminal.svelte.ts`.
 */
import { invoke } from "@tauri-apps/api/core";
import { tick } from "svelte";
import type { AppTab, Connection, SessionHistoryEntry } from "$lib/types";
import { notify } from "$lib/stores/notifications.svelte";
import {
  closeAllTabs,
  connectSSH,
  dockPopoutSession,
  fitActiveTerminal,
  focusActiveTerminal,
  getTerminalState,
  reattachSession,
  terminateAllDetachedSessions,
  terminatePopoutSession,
} from "$lib/stores/terminal.svelte";
import { getModalsState } from "$lib/stores/modals.svelte";
import { loadStats } from "$lib/stores/connections.svelte";

const terminalState = getTerminalState();
const modals = getModalsState();

let activeTab = $state<AppTab>("connections");
let sidebarCollapsed = $state(false);
let history = $state<SessionHistoryEntry[]>([]);

let showTerminalsPanel = $derived(
  activeTab === "terminals" ||
    terminalState.count > 0 ||
    terminalState.externalSessionCount > 0,
);

export async function loadHistory() {
  try {
    history = await invoke("get_history", { limit: 50 });
  } catch (e: unknown) {
    notify(String(e), "error");
  }
}

export function goToTerminals() {
  activeTab = "terminals";
  requestAnimationFrame(() => {
    fitActiveTerminal();
    focusActiveTerminal();
  });
}

export function handleTabChange(tab: AppTab) {
  activeTab = tab;
  if (tab === "terminals") {
    requestAnimationFrame(() => fitActiveTerminal());
  }
  if (tab === "history") {
    void loadHistory();
  }
}

export async function handleConnect(conn: Connection) {
  activeTab = "terminals";
  await tick();
  try {
    await connectSSH(conn);
    requestAnimationFrame(() => fitActiveTerminal());
  } catch (e: unknown) {
    notify(String(e), "error");
  }
}

export function openSessionManager() {
  modals.showSessionManager = true;
}

export async function handleSessionReattach(sessionId: string) {
  await reattachSession(sessionId);
  modals.showSessionManager = false;
  goToTerminals();
}

export async function handleSessionDock(sessionId: string) {
  await dockPopoutSession(sessionId);
  modals.showSessionManager = false;
  goToTerminals();
}

export async function handleTerminateAllSessions() {
  await terminateAllDetachedSessions();
  for (const session of [...terminalState.popoutSessions]) {
    await terminatePopoutSession(session.id);
  }
}

export async function terminateAllSessions() {
  const count = await closeAllTabs();
  if (count === 0) return;

  await loadHistory();
  await loadStats();
  notify(`Closed ${count} active session${count === 1 ? "" : "s"}`, "success");
}

export function requestCloseAllSessions() {
  const count = terminalState.totalSessionCount;
  if (count === 0) return;

  if (count === 1) {
    void terminateAllSessions();
    return;
  }

  modals.promptDelete(
    `${count} active or detached sessions`,
    "Every open SSH terminal will be disconnected immediately.",
    terminateAllSessions,
    {
      title: "Close all sessions",
      confirmLabel: "Close all",
      warning: "Unsaved work in remote shells may be lost.",
    },
  );
}

export function requestQuitApp() {
  const count = terminalState.totalSessionCount;
  const quit = async () => {
    await invoke("quit_app");
  };

  if (count === 0) {
    void quit();
    return;
  }

  modals.promptDelete(
    "Bayesian SSH",
    count === 1
      ? "1 active session will be closed and the application will exit."
      : `${count} active sessions will be closed and the application will exit.`,
    quit,
    {
      title: "Quit application",
      confirmLabel: "Quit",
      warning: "Unsaved work in remote shells may be lost.",
    },
  );
}

export function getSessionsState() {
  return {
    get activeTab() {
      return activeTab;
    },
    set activeTab(value: AppTab) {
      activeTab = value;
    },
    get sidebarCollapsed() {
      return sidebarCollapsed;
    },
    set sidebarCollapsed(value: boolean) {
      sidebarCollapsed = value;
    },
    get history() {
      return history;
    },
    set history(value: SessionHistoryEntry[]) {
      history = value;
    },
    get showTerminalsPanel() {
      return showTerminalsPanel;
    },
    loadHistory,
    goToTerminals,
    handleTabChange,
    handleConnect,
    openSessionManager,
    handleSessionReattach,
    handleSessionDock,
    handleTerminateAllSessions,
    terminateAllSessions,
    requestCloseAllSessions,
    requestQuitApp,
  };
}
