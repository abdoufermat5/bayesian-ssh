export const CONCRETE_THEMES = ["dark", "light"] as const;
export type ConcreteTheme = (typeof CONCRETE_THEMES)[number];

export const APP_THEMES = ["dark", "light", "system"] as const;
export type AppTheme = (typeof APP_THEMES)[number];

export const APP_DENSITIES = ["compact", "comfortable", "spacious"] as const;
export type AppDensity = (typeof APP_DENSITIES)[number];

/** The themes users can pick, in display order; onboarding and Appearance both read this. */
export const THEME_OPTIONS: { id: AppTheme; label: string; desc: string }[] = [
  { id: "light", label: "Light", desc: "Warm paper surfaces, amber accent" },
  { id: "dark", label: "Dark", desc: "Warm carbon surfaces, amber accent" },
  { id: "system", label: "Match system", desc: "Light or Dark, following your system" },
];

const THEME_WINDOW_BG: Record<ConcreteTheme, string> = {
  dark: "#0d0c0a",
  light: "#e6e1d6",
};

/** Themes from earlier versions, and what they became. */
const LEGACY_THEMES: Record<string, AppTheme> = {
  zinc: "dark",
  cyberpunk: "dark",
  oled: "dark",
  slate: "dark",
  paper: "light",
};

export function normalizeTheme(themeName: string): AppTheme {
  if (themeName in LEGACY_THEMES) return LEGACY_THEMES[themeName];
  return (APP_THEMES as readonly string[]).includes(themeName) ? (themeName as AppTheme) : "dark";
}

export function resolveSystemTheme(): ConcreteTheme {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "dark";
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function getResolvedTheme(themeName: string): ConcreteTheme {
  const theme = normalizeTheme(themeName);
  if (theme === "system") {
    return resolveSystemTheme();
  }
  return theme;
}

export function normalizeDensity(densityName?: string): AppDensity {
  if (densityName === "compact" || densityName === "spacious") {
    return densityName;
  }
  return "comfortable";
}

export function applyDensity(densityName?: string) {
  const density = normalizeDensity(densityName);
  if (typeof document !== "undefined") {
    document.documentElement.dataset.density = density;
  }
}

let systemThemeMediaQuery: MediaQueryList | null = null;
let systemThemeListener: ((e: MediaQueryListEvent) => void) | null = null;

export function applyTheme(themeName: string) {
  const theme = normalizeTheme(themeName);
  if (typeof document === "undefined") return;
  const root = document.documentElement;

  if (systemThemeMediaQuery && systemThemeListener) {
    systemThemeMediaQuery.removeEventListener("change", systemThemeListener);
    systemThemeMediaQuery = null;
    systemThemeListener = null;
  }

  if (theme === "system") {
    const applyResolved = () => {
      const resolved = resolveSystemTheme();
      for (const name of CONCRETE_THEMES) {
        root.classList.remove(`theme-${name}`);
      }
      root.classList.add(`theme-${resolved}`);
      root.dataset.theme = "system";
      root.dataset.resolvedTheme = resolved;
      void updateWindowBackground(resolved);
      window.dispatchEvent(new CustomEvent("bssh-theme-changed", { detail: { resolved } }));
    };

    applyResolved();

    if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
      systemThemeMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
      systemThemeListener = () => {
        applyResolved();
      };
      systemThemeMediaQuery.addEventListener("change", systemThemeListener);
    }
  } else {
    for (const name of CONCRETE_THEMES) {
      root.classList.remove(`theme-${name}`);
    }
    root.classList.add(`theme-${theme}`);
    root.dataset.theme = theme;
    root.dataset.resolvedTheme = theme;
    void updateWindowBackground(theme);
  }
}

async function updateWindowBackground(theme: ConcreteTheme) {
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

/** The terminal palette of the active theme (the --term-* tokens); Dark when unstyled. */
export function getCurrentXtermTheme(): XtermTheme {
  const fallback: XtermTheme = {
    background: "#0d0c0a",
    foreground: "#ddd6c8",
    cursor: "#f0a030",
    cursorAccent: "#0d0c0a",
    black: "#2c2924",
    red: "#e8685c",
    green: "#8fbf7a",
    yellow: "#d9b45a",
    blue: "#7fa6d0",
    magenta: "#c08fc0",
    cyan: "#6fb3ad",
    white: "#c9c1b2",
    brightBlack: "#6f685d",
    brightRed: "#f2837a",
    brightGreen: "#a8d493",
    brightYellow: "#ecd07a",
    brightBlue: "#9dbde0",
    brightMagenta: "#d7aed7",
    brightCyan: "#8ccbc4",
    brightWhite: "#f2ecdf",
    selectionBackground: "rgba(240, 160, 48, 0.28)",
  };

  if (typeof window === "undefined") {
    return fallback;
  }

  const style = getComputedStyle(document.documentElement);
  const read = (name: string, fb: string) => style.getPropertyValue(name).trim() || fb;

  return {
    background: read("--term-bg", fallback.background),
    foreground: read("--term-fg", fallback.foreground),
    cursor: read("--term-cursor", fallback.cursor),
    cursorAccent: read("--term-cursor-accent", fallback.cursorAccent),
    black: read("--term-black", fallback.black),
    red: read("--term-red", fallback.red),
    green: read("--term-green", fallback.green),
    yellow: read("--term-yellow", fallback.yellow),
    blue: read("--term-blue", fallback.blue),
    magenta: read("--term-magenta", fallback.magenta),
    cyan: read("--term-cyan", fallback.cyan),
    white: read("--term-white", fallback.white),
    brightBlack: read("--term-bright-black", fallback.brightBlack),
    brightRed: read("--term-bright-red", fallback.brightRed),
    brightGreen: read("--term-bright-green", fallback.brightGreen),
    brightYellow: read("--term-bright-yellow", fallback.brightYellow),
    brightBlue: read("--term-bright-blue", fallback.brightBlue),
    brightMagenta: read("--term-bright-magenta", fallback.brightMagenta),
    brightCyan: read("--term-bright-cyan", fallback.brightCyan),
    brightWhite: read("--term-bright-white", fallback.brightWhite),
    selectionBackground: read("--selection-bg", fallback.selectionBackground),
  };
}
