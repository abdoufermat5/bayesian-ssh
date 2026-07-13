export const APP_THEMES = ["zinc", "cyberpunk", "oled", "slate"] as const;
export type AppTheme = (typeof APP_THEMES)[number];

const THEME_WINDOW_BG: Record<AppTheme, string> = {
  zinc: "#09090b",
  cyberpunk: "#0c0813",
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
  cyan: string;
  magenta: string;
  green: string;
  red: string;
}

export function getCurrentXtermTheme(): XtermTheme {
  if (typeof window === "undefined") {
    return {
      background: "#0c0d12",
      foreground: "#cbd5e1",
      cursor: "#00f0ff",
      cursorAccent: "#0c0d12",
      cyan: "#00f0ff",
      magenta: "#d946ef",
      green: "#10b981",
      red: "#ef4444",
    };
  }

  const style = getComputedStyle(document.documentElement);
  const bg = style.getPropertyValue("--bg-terminal").trim() || "#0c0d12";
  const fg = style.getPropertyValue("--text-primary").trim() || "#cbd5e1";
  const accent = style.getPropertyValue("--accent-cyan").trim() || "#00f0ff";
  const pink = style.getPropertyValue("--accent-pink").trim() || "#d946ef";
  const green = style.getPropertyValue("--green-emerald").trim() || "#10b981";
  const red = style.getPropertyValue("--red-rose").trim() || "#ef4444";

  return {
    background: bg,
    foreground: fg,
    cursor: accent,
    cursorAccent: bg,
    cyan: accent,
    magenta: pink,
    green,
    red,
  };
}
