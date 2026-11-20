import { getCurrentXtermTheme } from "$lib/utils/theme";
import type { DesktopSettings } from "$lib/types";
import { getTerminalSettings } from "$lib/utils/terminal-options";
import { runtimes, scheduleFit } from "$lib/stores/terminal-runtime";

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
  for (const id of runtimes.keys()) scheduleFit(id);
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

/** Restore the size saved in Settings (or the default). */
export function resetTerminalFontSize() {
  updateTerminalFontSize(configuredFontSize ?? 13);
}
