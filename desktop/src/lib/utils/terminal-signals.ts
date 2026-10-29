import type { Terminal } from "@xterm/xterm";
import { ProgressAddon } from "@xterm/addon-progress";
import { getCurrentWindow, ProgressBarStatus, UserAttentionType } from "@tauri-apps/api/window";

export type ProgressState = 0 | 1 | 2 | 3 | 4; // OSC 9;4: 0 remove, 1 set, 2 error, 3 indeterminate, 4 paused

export interface TerminalSignalHandlers {
  onTitle(title: string): void;
  onBell(): void;
  onProgress(state: ProgressState, value: number): void; // value 0-100
}

/** Loads @xterm/addon-progress and wires term.onTitleChange / term.onBell / progress.onChange.
 *  Returns cleanup disposing all of them. */
export function attachXtermSignals(term: Terminal, handlers: TerminalSignalHandlers): () => void {
  const progressAddon = new ProgressAddon();
  term.loadAddon(progressAddon);

  const dTitle = term.onTitleChange((title: string) => {
    handlers.onTitle(title);
  });

  const dBell = term.onBell(() => {
    handlers.onBell();
  });

  const dProgress = progressAddon.onChange((event) => {
    handlers.onProgress(event.state, event.value);
  });

  return () => {
    try {
      dTitle.dispose();
    } catch {}
    try {
      dBell.dispose();
    } catch {}
    try {
      dProgress.dispose();
    } catch {}
    try {
      progressAddon.dispose();
    } catch {}
  };
}

/** Ask the OS for attention when the current window is not focused (Tauri
 *  getCurrentWindow().requestUserAttention(UserAttentionType.Informational)). */
export async function requestAttentionIfUnfocused(): Promise<void> {
  try {
    const win = getCurrentWindow();
    const focused = await win.isFocused();
    if (!focused) {
      await win.requestUserAttention(UserAttentionType.Informational);
    }
  } catch {
    // Swallow errors
  }
}

/** Mirror progress onto the OS taskbar/dock via getCurrentWindow().setProgressBar. */
export async function setWindowProgress(state: ProgressState, value: number): Promise<void> {
  try {
    let status: ProgressBarStatus;
    switch (state) {
      case 0:
        status = ProgressBarStatus.None;
        break;
      case 1:
        status = ProgressBarStatus.Normal;
        break;
      case 2:
        status = ProgressBarStatus.Error;
        break;
      case 3:
        status = ProgressBarStatus.Indeterminate;
        break;
      case 4:
        status = ProgressBarStatus.Paused;
        break;
      default:
        status = ProgressBarStatus.None;
        break;
    }

    const clamped = Math.max(0, Math.min(100, Math.round(Number.isFinite(value) ? value : 0)));
    await getCurrentWindow().setProgressBar({
      status,
      progress: clamped,
    });
  } catch {
    // Swallow errors
  }
}
