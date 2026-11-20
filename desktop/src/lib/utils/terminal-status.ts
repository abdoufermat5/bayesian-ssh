import type { Connection } from "$lib/types";
import type { TerminalTabStatus } from "$lib/stores/terminal.svelte";

/** `status-dot-*` class for a tab's connection state. */
export function statusDot(status: TerminalTabStatus): string {
  switch (status) {
    case "connected":
      return "status-dot-success";
    case "connecting":
      return "status-dot-warning";
    case "error":
      return "status-dot-error";
    default:
      return "status-dot-offline";
  }
}

export function statusLabel(status: TerminalTabStatus): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "connecting":
      return "Connecting";
    case "error":
      return "Failed to connect";
    default:
      return "Disconnected";
  }
}

/** `user@host[:port]`, the port only when it isn't 22. */
export function hostLine(conn: Connection): string {
  return `${conn.user}@${conn.host}${conn.port !== 22 ? `:${conn.port}` : ""}`;
}

/** Svelte action: focus (and select) an input once it is laid out. */
export function focusOnMount(node: HTMLInputElement) {
  requestAnimationFrame(() => {
    node.focus();
    node.select();
  });
}
