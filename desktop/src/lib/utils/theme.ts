export const APP_THEMES = ["zinc", "cyberpunk", "oled", "slate"] as const;
export type AppTheme = (typeof APP_THEMES)[number];

const THEME_WINDOW_BG: Record<AppTheme, string> = {
  zinc: "#101416",
  cyberpunk: "#061724",
  oled: "#000000",
  slate: "#0f172a",
};

export function normalizeTheme(themeName: string): AppTheme {
  return APP_THEMES.includes(themeName as AppTheme) ? (themeName as AppTheme) : "zinc";
}

export function applyTheme(themeName: string) {
  const theme = normalizeTheme(themeName);
  const root = document.documentElement;

  for (const name of APP_THEMES) {
    root.classList.remove(`theme-${name}`);
  }
  root.classList.add(`theme-${theme}`);
  root.dataset.theme = theme;

  // Keep native window chrome in sync (Tauri)
  void updateWindowBackground(theme);
}

async function updateWindowBackground(theme: AppTheme) {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const hex = THEME_WINDOW_BG[theme];
    const r = parseInt(hex.slice(1, 3), 16);
    const g = parseInt(hex.slice(3, 5), 16);
    const b = parseInt(hex.slice(5, 7), 16);
    await getCurrentWindow().setBackgroundColor({ red: r, green: g, blue: b, alpha: 255 });
  } catch {
    // Browser preview / non-Tauri context
  }
}

export interface XtermTheme {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
  selectionBackground: string;
}

export function getCurrentXtermTheme(): XtermTheme {
  const fallback: XtermTheme = {
    background: "#0c0d12",
    foreground: "#cbd5e1",
    cursor: "#00f0ff",
    cursorAccent: "#0c0d12",
    black: "#18181b",
    red: "#ef4444",
    green: "#10b981",
    yellow: "#fbbf24",
    blue: "#3b82f6",
    magenta: "#d946ef",
    cyan: "#00f0ff",
    white: "#cbd5e1",
    brightBlack: "#71717a",
    brightRed: "#f87171",
    brightGreen: "#34d399",
    brightYellow: "#fde047",
    brightBlue: "#60a5fa",
    brightMagenta: "#e879f9",
    brightCyan: "#22d3ee",
    brightWhite: "#f4f4f5",
    selectionBackground: "rgba(59, 130, 246, 0.25)",
  };

  if (typeof window === "undefined") {
    return fallback;
  }

  const style = getComputedStyle(document.documentElement);
  const read = (name: string, fb: string) => style.getPropertyValue(name).trim() || fb;

  return {
    background: read("--bg-terminal", fallback.background),
    foreground: read("--text-primary", fallback.foreground),
    cursor: read("--accent-cyan", fallback.cursor),
    cursorAccent: read("--bg-terminal", fallback.cursorAccent),
    black: read("--surface-input", fallback.black),
    red: read("--red-rose", fallback.red),
    green: read("--green-emerald", fallback.green),
    yellow: read("--yellow-amber", fallback.yellow),
    blue: read("--accent-blue", fallback.blue),
    magenta: read("--accent-pink", fallback.magenta),
    cyan: read("--accent-cyan", fallback.cyan),
    white: read("--text-secondary", fallback.white),
    brightBlack: read("--text-muted", fallback.brightBlack),
    brightRed: read("--red-rose", fallback.brightRed),
    brightGreen: read("--green-emerald", fallback.brightGreen),
    brightYellow: read("--yellow-amber", fallback.brightYellow),
    brightBlue: read("--accent-blue", fallback.brightBlue),
    brightMagenta: read("--accent-pink", fallback.brightMagenta),
    brightCyan: read("--accent-cyan", fallback.brightCyan),
    brightWhite: read("--text-primary", fallback.brightWhite),
    selectionBackground: read("--selection-bg", fallback.selectionBackground),
  };
}
