import { getCurrentWindow } from "@tauri-apps/api/window";

let isFullscreen = $state(false);
let isMaximized = $state(false);
let initialized = false;

async function syncWindowState() {
  const win = getCurrentWindow();
  isFullscreen = await win.isFullscreen();
  isMaximized = await win.isMaximized();
}

export async function initWindowState(): Promise<() => void> {
  if (initialized) return () => {};
  initialized = true;

  await syncWindowState();

  const win = getCurrentWindow();
  const unlisten = await win.onResized(() => {
    syncWindowState();
  });

  return () => {
    unlisten();
    initialized = false;
  };
}

/** Call after manual maximize/unmaximize/fullscreen toggles. */
export async function refreshWindowState() {
  await syncWindowState();
}

export function getWindowState() {
  return {
    get isFullscreen() {
      return isFullscreen;
    },
    get isMaximized() {
      return isMaximized;
    },
  };
}
