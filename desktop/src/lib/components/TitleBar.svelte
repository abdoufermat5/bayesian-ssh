<script lang="ts">
  import { Command, HelpCircle, Terminal } from "lucide-svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { refreshWindowState } from "$lib/stores/window.svelte";

  interface Props {
    activeEnv: string;
    onOpenAbout?: () => void;
    onOpenShortcuts?: () => void;
    onOpenCommandPalette?: () => void;
  }

  let { activeEnv, onOpenAbout, onOpenShortcuts, onOpenCommandPalette }: Props = $props();

  const appWindow = getCurrentWindow();

  async function handleWindowMinimize() {
    try {
      await appWindow.minimize();
    } catch (e) {
      console.error(e);
    }
  }

  async function handleWindowMaximize() {
    try {
      if (await appWindow.isMaximized()) {
        await appWindow.unmaximize();
      } else {
        await appWindow.maximize();
      }
      await refreshWindowState();
    } catch (e) {
      console.error(e);
    }
  }

  async function handleWindowClose() {
    try {
      await appWindow.close();
    } catch (e) {
      console.error(e);
    }
  }
</script>

<header class="window-titlebar z-50">
  <!-- Left Brand & App Title -->
  <div
    class="flex items-center shrink-0 h-full cursor-default gap-2"
    data-tauri-drag-region
  >
    <div class="brand-mark">
      <Terminal size={11} class="text-secondary" />
    </div>
    <span class="font-semibold text-xs text-primary tracking-tight select-none pointer-events-none">
      Bayesian SSH
    </span>
    <span class="font-mono text-[10px] text-muted select-none pointer-events-none">
      v2.5.2
    </span>
  </div>

  <!-- Center Profile Pill -->
  <div
    class="hidden md:flex justify-center items-center flex-1 min-w-0 h-full cursor-default"
    data-tauri-drag-region
  >
    <div class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full border border-border bg-surface-input text-2xs text-secondary select-none">
      <span class="w-1.5 h-1.5 rounded-full bg-success shrink-0"></span>
      <span class="text-muted">Profile:</span>
      <span class="text-primary font-medium truncate max-w-[130px]" title={activeEnv}>{activeEnv}</span>
    </div>
  </div>

  <!-- Right Actions & Native Window Controls -->
  <div class="flex items-center justify-end shrink-0 h-full gap-0.5 ml-auto">
    {#if onOpenShortcuts}
      <button
        type="button"
        class="btn-icon h-7 w-7 p-0"
        onclick={onOpenShortcuts}
        title="Keyboard Shortcuts (F1 or ?)"
        aria-label="Keyboard Shortcuts"
      >
        <Command size={12} />
      </button>
    {/if}

    {#if onOpenAbout}
      <button
        type="button"
        class="btn-icon h-7 w-7 p-0"
        onclick={onOpenAbout}
        title="About Bayesian SSH"
        aria-label="About Bayesian SSH"
      >
        <HelpCircle size={13} />
      </button>
    {/if}

    <!-- Window buttons -->
    <div class="flex items-center ml-1.5 border-l border-border/60 pl-1">
      <button
        type="button"
        class="btn-icon h-[var(--titlebar-h)] w-8 rounded-none p-0 hover:bg-surface-hover"
        onclick={handleWindowMinimize}
        title="Minimize"
        aria-label="Minimize"
      >
        <svg viewBox="0 0 10 1" class="w-2.5 h-px fill-none stroke-current" style="stroke-width: 1.5;">
          <line x1="0" y1="0.5" x2="10" y2="0.5" />
        </svg>
      </button>
      <button
        type="button"
        class="btn-icon h-[var(--titlebar-h)] w-8 rounded-none p-0 hover:bg-surface-hover"
        onclick={handleWindowMaximize}
        title="Maximize/Restore"
        aria-label="Maximize/Restore"
      >
        <svg viewBox="0 0 10 10" class="w-2.5 h-2.5 fill-none stroke-current" style="stroke-width: 1.2;">
          <rect x="1" y="1" width="8" height="8" />
        </svg>
      </button>
      <button
        type="button"
        class="btn-icon h-[var(--titlebar-h)] w-8 rounded-none p-0 hover:bg-error hover:text-white"
        onclick={handleWindowClose}
        title="Close"
        aria-label="Close"
      >
        <svg viewBox="0 0 10 10" class="w-2.5 h-2.5 fill-none stroke-current" style="stroke-width: 1.2;">
          <path d="M1 1 L9 9 M9 1 L1 9" />
        </svg>
      </button>
    </div>
  </div>
</header>
