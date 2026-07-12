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
