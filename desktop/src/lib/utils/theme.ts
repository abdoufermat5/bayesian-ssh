export const CONCRETE_THEMES = ["zinc", "cyberpunk", "oled", "paper"] as const;
export type ConcreteTheme = (typeof CONCRETE_THEMES)[number];

export const APP_THEMES = ["zinc", "cyberpunk", "oled", "paper", "system"] as const;
export type AppTheme = (typeof APP_THEMES)[number];

export const APP_DENSITIES = ["compact", "comfortable", "spacious"] as const;
export type AppDensity = (typeof APP_DENSITIES)[number];

/** The themes users can pick, in display order; onboarding and Appearance both read this. */
export const THEME_OPTIONS: { id: AppTheme; label: string; desc: string }[] = [
  { id: "zinc", label: "Graphite", desc: "Neutral gray, blue accent" },
  { id: "cyberpunk", label: "Midnight", desc: "Deep navy, cyan accent" },
  { id: "oled", label: "OLED black", desc: "Pure black for OLED screens" },
  { id: "paper", label: "Paper", desc: "Warm light theme, crisp contrast" },
  { id: "system", label: "Match system", desc: "Paper or Graphite from system preference" },
];

const THEME_WINDOW_BG: Record<ConcreteTheme, string> = {
  zinc: "#0b0c0e",
  cyberpunk: "#070b14",
  oled: "#000000",
  paper: "#ebebe8",
};

export function normalizeTheme(themeName: string): (typeof APP_THEMES)[number] {
  if (themeName === "slate") {
    return "cyberpunk";
  }
  return (APP_THEMES as readonly string[]).includes(themeName)
    ? (themeName as (typeof APP_THEMES)[number])
    : "zinc";
}

export function resolveSystemTheme(): ConcreteTheme {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "zinc";
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "zinc" : "paper";
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
    background: read("--term-bg", read("--bg-terminal", fallback.background)),
    foreground: read("--term-fg", read("--text-primary", fallback.foreground)),
    cursor: read("--term-cursor", read("--accent-cyan", fallback.cursor)),
    cursorAccent: read("--term-cursor-accent", read("--bg-terminal", fallback.cursorAccent)),
    black: read("--term-black", read("--surface-input", fallback.black)),
    red: read("--term-red", read("--red-rose", fallback.red)),
    green: read("--term-green", read("--green-emerald", fallback.green)),
    yellow: read("--term-yellow", read("--yellow-amber", fallback.yellow)),
    blue: read("--term-blue", read("--accent-blue", fallback.blue)),
    magenta: read("--term-magenta", read("--accent-pink", fallback.magenta)),
    cyan: read("--term-cyan", read("--accent-cyan", fallback.cyan)),
    white: read("--term-white", read("--text-secondary", fallback.white)),
    brightBlack: read("--term-bright-black", read("--text-muted", fallback.brightBlack)),
    brightRed: read("--term-bright-red", read("--red-rose", fallback.brightRed)),
    brightGreen: read("--term-bright-green", read("--green-emerald", fallback.brightGreen)),
    brightYellow: read("--term-bright-yellow", read("--yellow-amber", fallback.brightYellow)),
    brightBlue: read("--term-bright-blue", read("--accent-blue", fallback.brightBlue)),
    brightMagenta: read("--term-bright-magenta", read("--accent-pink", fallback.brightMagenta)),
    brightCyan: read("--term-bright-cyan", read("--accent-cyan", fallback.brightCyan)),
    brightWhite: read("--term-bright-white", read("--text-primary", fallback.brightWhite)),
    selectionBackground: read("--selection-bg", fallback.selectionBackground),
  };
}
