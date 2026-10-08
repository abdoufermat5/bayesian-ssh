<script lang="ts">
  import { Command, HelpCircle, Search } from "lucide-svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  interface Props {
    onOpenAbout?: () => void;
    onOpenShortcuts?: () => void;
    onOpenCommandPalette?: () => void;
  }

  let { onOpenAbout, onOpenShortcuts, onOpenCommandPalette }: Props = $props();

  const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);

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
      await appWindow.toggleMaximize();
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

<header class="titlebar" data-tauri-drag-region>
  <div class="flex min-w-0 items-center gap-2.5" data-tauri-drag-region>
    <svg viewBox="0 0 20 20" class="size-[18px] shrink-0" aria-hidden="true">
      <rect width="20" height="20" rx="5" fill="var(--color-accent)" />
      <path d="M5.5 7l3 3-3 3" fill="none" stroke="var(--color-on-accent)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      <path d="M10.5 13.25h4" stroke="var(--color-on-accent)" stroke-width="1.8" stroke-linecap="round" />
    </svg>
    <span class="pointer-events-none truncate text-sm font-semibold tracking-[-0.01em] text-primary">
      Bayesian SSH
    </span>
  </div>

  <div class="flex min-w-0 justify-center" data-tauri-drag-region>
    {#if onOpenCommandPalette}
      <button
        type="button"
        class="flex h-[26px] w-full max-w-[420px] cursor-pointer items-center gap-2 rounded-md border border-border bg-surface px-2.5 text-xs text-muted transition-colors duration-fast hover:border-border-hover hover:text-secondary"
        onclick={onOpenCommandPalette}
        aria-label="Open command palette"
      >
        <Search size={13} class="shrink-0" />
        <span class="truncate">Search hosts and commands</span>
        <span class="ml-auto flex shrink-0 items-center gap-0.5">
          <kbd class="kbd">{isMac ? "⌘" : "Ctrl"}</kbd>
          <kbd class="kbd">K</kbd>
        </span>
      </button>
    {/if}
  </div>

  <div class="flex h-full items-center justify-end" data-tauri-drag-region>
    {#if onOpenShortcuts}
      <button
        type="button"
        class="btn-icon"
        onclick={onOpenShortcuts}
        title="Keyboard shortcuts (?)"
        aria-label="Keyboard shortcuts"
      >
        <Command size={14} />
      </button>
    {/if}
    {#if onOpenAbout}
      <button
        type="button"
        class="btn-icon mr-1"
        onclick={onOpenAbout}
        title="About Bayesian SSH"
        aria-label="About Bayesian SSH"
      >
        <HelpCircle size={14} />
      </button>
    {/if}

    <div class="flex h-full items-stretch">
      <button
        type="button"
        class="window-btn"
        onclick={handleWindowMinimize}
        title="Minimize"
        aria-label="Minimize"
      >
        <svg viewBox="0 0 10 10" class="size-2.5" fill="none" stroke="currentColor" stroke-width="1.2">
          <path d="M1 5.5h8" />
        </svg>
      </button>
      <button
        type="button"
        class="window-btn"
        onclick={handleWindowMaximize}
        title="Maximize"
        aria-label="Maximize or restore"
      >
        <svg viewBox="0 0 10 10" class="size-2.5" fill="none" stroke="currentColor" stroke-width="1.2">
          <rect x="1.5" y="1.5" width="7" height="7" rx="1" />
        </svg>
      </button>
      <button
        type="button"
        class="window-btn window-btn-close"
        onclick={handleWindowClose}
        title="Close"
        aria-label="Close"
      >
        <svg viewBox="0 0 10 10" class="size-2.5" fill="none" stroke="currentColor" stroke-width="1.2">
          <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" />
        </svg>
      </button>
    </div>
  </div>
</header>

<style>
  .window-btn {
    display: inline-flex;
    width: 44px;
    align-items: center;
    justify-content: center;
    color: var(--color-muted);
    cursor: pointer;
    transition: background-color 100ms, color 100ms;
  }
  .window-btn:hover {
    background: var(--color-surface-hover);
    color: var(--color-primary);
  }
  .window-btn-close:hover {
    background: #e5484d;
    color: #fff;
  }
</style>
