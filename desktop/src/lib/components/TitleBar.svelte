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

<header
  class="flex shrink-0 items-center justify-between h-[var(--titlebar-h)] bg-surface border-b border-border/80 px-3 select-none z-50 text-xs"
>
  <!-- Left Brand & App Title -->
  <div
    class="flex items-center flex-1 h-full cursor-default gap-2.5"
    data-tauri-drag-region
  >
    <div class="flex items-center justify-center w-5 h-5 rounded-md bg-accent/15 border border-accent/30 text-accent shrink-0">
      <Terminal size={12} />
    </div>
    <span class="font-semibold text-primary tracking-tight select-none pointer-events-none">
      Bayesian SSH
    </span>
    <span class="text-[10px] text-muted font-mono px-1.5 py-0.2 rounded bg-surface-input border border-border">
      v2.5.1
    </span>
  </div>

  <!-- Center Profile Pill -->
  <div
    class="flex justify-center items-center flex-1 h-full cursor-default"
    data-tauri-drag-region
  >
    <div
      class="bg-surface-input/80 border border-border rounded-full px-3 py-0.5 text-[11px] text-secondary flex items-center gap-1.5 shadow-sm"
    >
      <span class="w-1.5 h-1.5 rounded-full bg-running shrink-0"></span>
      <span class="text-muted">Profile:</span>
      <span class="text-primary font-semibold truncate max-w-[140px]">{activeEnv}</span>
    </div>
  </div>

  <!-- Right Actions & Native Controls -->
  <div class="flex items-center justify-end flex-1 h-full gap-1">
    {#if onOpenCommandPalette}
      <button
        type="button"
        class="flex items-center gap-1 px-2 h-6 bg-surface-input/60 border border-border text-muted hover:text-primary hover:border-border-hover transition-colors rounded-md text-[11px] cursor-pointer"
        onclick={onOpenCommandPalette}
        title="Open Command Palette (Ctrl+K)"
        aria-label="Open Command Palette"
      >
        <Command size={11} />
        <span class="font-mono text-[10px]">⌘K</span>
      </button>
    {/if}

    {#if onOpenShortcuts}
      <button
        type="button"
        class="flex items-center justify-center w-7 h-7 bg-transparent border-none text-muted hover:text-primary hover:bg-surface-hover transition-colors rounded-md cursor-pointer"
        onclick={onOpenShortcuts}
        title="Keyboard Shortcuts"
        aria-label="Keyboard Shortcuts"
      >
        <Command size={13} />
      </button>
    {/if}

    {#if onOpenAbout}
      <button
        type="button"
        class="flex items-center justify-center w-7 h-7 bg-transparent border-none text-muted hover:text-primary hover:bg-surface-hover transition-colors rounded-md cursor-pointer"
        onclick={onOpenAbout}
        title="About Bayesian SSH"
        aria-label="About Bayesian SSH"
      >
        <HelpCircle size={14} />
      </button>
    {/if}

    <!-- Window buttons -->
    <div class="flex items-center ml-2 border-l border-border/60 pl-1">
      <button
        type="button"
        class="flex items-center justify-center w-8 h-[var(--titlebar-h)] bg-transparent border-none text-muted hover:bg-surface-hover hover:text-primary transition-colors cursor-pointer"
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
        class="flex items-center justify-center w-8 h-[var(--titlebar-h)] bg-transparent border-none text-muted hover:bg-surface-hover hover:text-primary transition-colors cursor-pointer"
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
        class="flex items-center justify-center w-8 h-[var(--titlebar-h)] bg-transparent border-none text-muted hover:bg-error hover:text-white transition-colors cursor-pointer"
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
