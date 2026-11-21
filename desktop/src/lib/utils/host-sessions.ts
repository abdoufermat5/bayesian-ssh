/**
 * Per-host session rollup for the Connections patch bay: how many terminal
 * tabs point at each host, and the strongest state among them. The lamp on a
 * host row lights amber while any session to it is live.
 */
import { getTerminalState, type TerminalTabStatus } from "$lib/stores/terminal.svelte";

/** Lamp state of one host's sessions, most demanding first. */
export type HostSessionStatus = "idle" | "live" | "connecting" | "error";

export interface HostSessionState {
  /** Terminal tabs open against this host, whatever their status. */
  count: number;
  status: HostSessionStatus;
}

/** A live session outranks a connecting one, which outranks a failed one. */
const RANK: Record<HostSessionStatus, number> = { idle: 0, error: 1, connecting: 2, live: 3 };

/** Shared zero-session value so every idle row reads the same object. */
export const IDLE_HOST_SESSION: HostSessionState = { count: 0, status: "idle" };

function tabStatusOf(status: TerminalTabStatus): HostSessionStatus {
  switch (status) {
    case "connected":
      return "live";
    case "connecting":
      return "connecting";
    case "error":
      return "error";
    default:
      // "exited": the session is over; it still counts, but the jack is dark.
      return "idle";
  }
}

/** Reads reactive store state: call it inside `$derived` or a template. */
export function hostSessionState(connId: string): HostSessionState {
  let count = 0;
  let status: HostSessionStatus = "idle";
  for (const tab of getTerminalState().tabs) {
    if (tab.conn?.id !== connId) continue;
    count += 1;
    const s = tabStatusOf(tab.status);
    if (RANK[s] > RANK[status]) status = s;
  }
  return { count, status };
}

/**
 * Same rollup for a whole list, in one pass over the tabs: rows look their
 * host up in the map instead of each scanning every tab.
 * Reads reactive store state: call it inside `$derived` or a template.
 */
export function sessionsByHost(): Map<string, HostSessionState> {
  const map = new Map<string, HostSessionState>();
  for (const tab of getTerminalState().tabs) {
    const connId = tab.conn?.id;
    if (!connId) continue;
    const status = tabStatusOf(tab.status);
    const prev = map.get(connId);
    if (!prev) {
      map.set(connId, { count: 1, status });
    } else {
      prev.count += 1;
      if (RANK[status] > RANK[prev.status]) prev.status = status;
    }
  }
  return map;
}
