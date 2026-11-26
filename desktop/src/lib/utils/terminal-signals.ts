import type { Terminal } from "@xterm/xterm";
import { ProgressAddon } from "@xterm/addon-progress";
import type { ProgressState } from "$lib/utils/window-signals";
export {
  requestAttentionIfUnfocused,
  setWindowProgress,
  type ProgressState,
} from "$lib/utils/window-signals";

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
