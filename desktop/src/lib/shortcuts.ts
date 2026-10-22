import type { AppTab } from "$lib/types";

/**
 * The one shortcut table. The global key handler, the sidebar hints, the
 * shortcut sheet and the command palette all read it; components that own a
 * key (the host list, terminal tabs) implement exactly what is listed here.
 * Never mirror a shortcut by hand anywhere else.
 *
 * Rule: plain Ctrl/Cmd chords belong to the shell inside a terminal; app
 * chords there use Ctrl+Shift (Cmd on macOS).
 */

export const IS_MAC =
  typeof navigator !== "undefined" && /mac/i.test(navigator.platform || navigator.userAgent);

/** Display name of the primary modifier. */
export const MOD = IS_MAC ? "⌘" : "Ctrl";
/** Modifier set for app chords inside a terminal. */
export const TERM_MOD: string[] = IS_MAC ? ["⌘"] : ["Ctrl", "Shift"];

/** A key combination, e.g. ["Ctrl", "K"]; a shortcut may have several. */
export type Combo = string[];

export interface Shortcut {
  id: string;
  label: string;
  combos: Combo[];
  /** Where it works. "global" means outside text inputs and terminals. */
  scope: "global" | "anywhere" | "hosts" | "terminal";
  /** CLI equivalent shown in the palette, when there is one. */
  cli?: string;
}

/** Sidebar order; digit n opens the nth view (outside inputs and terminals). */
export const VIEW_ORDER: AppTab[] = [
  "connections",
  "terminals",
  "sftp",
  "tunnels",
  "keys",
  "audit",
  "history",
  "teams",
];

export function viewDigit(tab: AppTab): string | null {
  const i = VIEW_ORDER.indexOf(tab);
  return i === -1 ? null : String(i + 1);
}

export const GLOBAL_SHORTCUTS: Shortcut[] = [
  { id: "palette", label: "Command palette", combos: [[MOD, "K"]], scope: "anywhere" },
  ...(IS_MAC
    ? []
    : [
        {
          id: "palette-terminal",
          label: "Command palette, inside a terminal",
          combos: [[MOD, "Shift", "P"]],
          scope: "terminal" as const,
        },
      ]),
  { id: "settings", label: "Settings", combos: [[MOD, ","]], scope: "anywhere" },
  { id: "filter", label: "Focus filter", combos: [["/"]], scope: "global" },
  { id: "new-host", label: "New host", combos: [["N"], [MOD, "N"]], scope: "global", cli: "bssh add" },
];

/** On the Hosts list, with a host selected. */
export const HOST_SHORTCUTS: Shortcut[] = [
  { id: "host-connect", label: "Connect", combos: [["Enter"]], scope: "hosts", cli: "bssh connect <host>" },
  { id: "host-connect-tab", label: "Connect in a new tab", combos: [["Shift", "Enter"]], scope: "hosts" },
  { id: "host-files", label: "Browse files", combos: [[MOD, "Enter"]], scope: "hosts" },
  { id: "host-copy", label: "Copy ssh command", combos: [["C"]], scope: "hosts" },
  { id: "host-edit", label: "Edit", combos: [["E"]], scope: "hosts", cli: "bssh edit <host>" },
  { id: "host-share", label: "Share with a team…", combos: [["S"]], scope: "hosts", cli: "bssh teams share <host> --team <team>" },
  { id: "host-delete", label: "Delete…", combos: [["Delete"]], scope: "hosts", cli: "bssh remove <host>" },
  { id: "host-move", label: "Move selection", combos: [["↑"], ["↓"], ["J"], ["K"]], scope: "hosts" },
];

/** In the Terminals view; work while the terminal has focus. */
export const TERMINAL_SHORTCUTS: Shortcut[] = [
  { id: "term-next-tab", label: "Next tab", combos: [["Ctrl", "Tab"]], scope: "terminal" },
  { id: "term-prev-tab", label: "Previous tab", combos: [["Ctrl", "Shift", "Tab"]], scope: "terminal" },
  { id: "term-close-tab", label: "Close tab", combos: [[...TERM_MOD, "W"]], scope: "terminal" },
  { id: "term-reopen-tab", label: "Reopen closed tab", combos: [[...TERM_MOD, "T"]], scope: "terminal" },
  { id: "term-copy", label: "Copy", combos: [[...TERM_MOD, "C"]], scope: "terminal" },
  { id: "term-paste", label: "Paste", combos: [[...TERM_MOD, "V"]], scope: "terminal" },
  { id: "term-find", label: "Find", combos: [[...TERM_MOD, "F"]], scope: "terminal" },
  { id: "term-zoom-in", label: "Larger text", combos: [[...TERM_MOD, "="]], scope: "terminal" },
  { id: "term-zoom-out", label: "Smaller text", combos: [[...TERM_MOD, "-"]], scope: "terminal" },
  { id: "term-zoom-reset", label: "Reset text size", combos: [[...TERM_MOD, "0"]], scope: "terminal" },
  { id: "term-cancel", label: "Cancel a connection in progress", combos: [["Esc"]], scope: "terminal" },
];
