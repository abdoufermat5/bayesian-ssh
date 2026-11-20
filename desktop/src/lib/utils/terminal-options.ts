import type { DesktopSettings } from "$lib/types";
import { getCurrentXtermTheme } from "$lib/utils/theme";

export type SettingsGetter = () => DesktopSettings | undefined;
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
