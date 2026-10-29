import type { Terminal } from "@xterm/xterm";
import { mount, unmount } from "svelte";
import TerminalContextMenu from "$lib/components/terminal/TerminalContextMenu.svelte";

export interface TerminalMenuActions {
  paste(): void;          // CORE supplies: reads clipboard then pasteIntoTerminal
  find(): void;           // opens the search bar
  exportScrollback(): void;
}

let activeMenuClose: (() => void) | null = null;

/**
 * contextmenu listener on container. If the app enabled mouse tracking
 * (term.modes.mouseTrackingMode !== "none") and shift is NOT held: do nothing besides
 * preventDefault (xterm forwards the click to the app). Otherwise preventDefault and open
 * TerminalContextMenu (mounted imperatively into document.body) at the pointer, clamped to
 * the viewport. Items: Copy (disabled without selection; uses copyTextWithFallback from
 * terminal-xterm.ts), Paste, Select all, Find, Clear (term.clear()), Export scrollback.
 * Keyboard: arrows/Home/End/Enter/Escape; closes on outside click, blur, scroll, resize.
 * Returns cleanup (closes an open menu).
 */
export function attachTerminalContextMenu(
  container: HTMLElement,
  term: Terminal,
  actions: TerminalMenuActions
): () => void {
  if (!container || !term) return () => {};

  let currentMenuClose: (() => void) | null = null;

  const openMenu = (x: number, y: number) => {
    // Only one menu open at a time globally
    if (activeMenuClose) {
      activeMenuClose();
      activeMenuClose = null;
    }

    let instance: Record<string, unknown> | null = null;
    let isClosed = false;

    const closeThisMenu = (refocusTerminal = false) => {
      if (isClosed) return;
      isClosed = true;

      if (activeMenuClose === currentMenuClose) {
        activeMenuClose = null;
      }
      currentMenuClose = null;

      if (instance) {
        try {
          unmount(instance);
        } catch (err) {
          console.error("Failed to unmount TerminalContextMenu", err);
        }
        instance = null;
      }

      if (refocusTerminal) {
        try {
          term.focus();
        } catch {
          // terminal may be disposed
        }
      }
    };

    const closeWithoutRefocus = () => closeThisMenu(false);
    activeMenuClose = closeWithoutRefocus;
    currentMenuClose = closeWithoutRefocus;

    try {
      instance = mount(TerminalContextMenu, {
        target: document.body,
        props: {
          x,
          y,
          term,
          actions,
          onClose: closeThisMenu,
        },
      });
    } catch (err) {
      console.error("Failed to mount TerminalContextMenu", err);
      closeThisMenu(false);
    }
  };

  const onContextMenu = (e: MouseEvent) => {
    const trackingMode = term.modes?.mouseTrackingMode;
    const isMouseTracking = Boolean(trackingMode && trackingMode !== "none");

    // If the app enabled mouse tracking and shift is NOT held: do nothing besides preventDefault
    if (isMouseTracking && !e.shiftKey) {
      e.preventDefault();
      return;
    }

    e.preventDefault();

    let clientX = e.clientX;
    let clientY = e.clientY;
    if (clientX === 0 && clientY === 0 && container) {
      const rect = container.getBoundingClientRect();
      clientX = rect.left + Math.min(rect.width / 2, 80);
      clientY = rect.top + Math.min(rect.height / 2, 80);
    }

    openMenu(clientX, clientY);
  };

  container.addEventListener("contextmenu", onContextMenu);

  return () => {
    container.removeEventListener("contextmenu", onContextMenu);
    if (currentMenuClose) {
      currentMenuClose();
      currentMenuClose = null;
    }
  };
}
