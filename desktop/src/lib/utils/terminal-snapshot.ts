import type { Terminal } from "@xterm/xterm";
import { SerializeAddon } from "@xterm/addon-serialize";

const addonMap = new WeakMap<Terminal, SerializeAddon>();

/**
 * Load SerializeAddon once per terminal (cache in a WeakMap).
 * Safe against double-registration and never throws.
 */
export function enableSnapshot(term: Terminal): void {
  try {
    if (!term || addonMap.has(term)) return;
    const addon = new SerializeAddon();
    term.loadAddon(addon);
    addonMap.set(term, addon);
  } catch (err) {
    console.error("Failed to enable terminal snapshot", err);
  }
}

/**
 * Serialize the full state incl. modes and alt buffer (scrollback capped to the
 * terminal's scrollback option); returns null if the addon was never enabled or on error.
 * Never throws.
 */
export function snapshotTerminal(term: Terminal): string | null {
  try {
    if (!term) return null;
    const addon = addonMap.get(term);
    if (!addon) return null;
    return addon.serialize({
      scrollback: term.options?.scrollback,
      excludeModes: false,
      excludeAltBuffer: false,
    });
  } catch (err) {
    console.error("Failed to snapshot terminal", err);
    return null;
  }
}
