<script lang="ts">
  import BrandMark from "./BrandMark.svelte";
  import { Info, Keyboard, Search } from "lucide-svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { GLOBAL_SHORTCUTS, IS_MAC } from "$lib/shortcuts";

  interface Props {
    onOpenAbout?: () => void;
    onOpenShortcuts?: () => void;
    onOpenCommandPalette?: () => void;
  }

  let { onOpenAbout, onOpenShortcuts, onOpenCommandPalette }: Props = $props();

  const paletteKeys = GLOBAL_SHORTCUTS.find((s) => s.id === "palette")?.combos[0] ?? [];

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
    <BrandMark class="size-[18px] shrink-0" />
    <span
      class="pointer-events-none truncate font-display text-sm font-semibold uppercase tracking-[0.04em] text-primary"
      data-tauri-drag-region
    >
      Bayesian SSH
    </span>
  </div>

  <div class="flex min-w-0 justify-center" data-tauri-drag-region>
    {#if onOpenCommandPalette}
      <button
        type="button"
        class="flex h-[26px] w-full max-w-[440px] cursor-pointer items-center gap-2 rounded-md border border-border-control bg-surface-input px-2.5 text-xs text-muted transition-colors duration-fast hover:border-border-hover hover:text-secondary"
        onclick={onOpenCommandPalette}
        aria-label="Jump to host, view or action"
        title="Jump to hosts, views and actions"
      >
        <Search size={13} class="shrink-0" aria-hidden="true" />
        <span class="mono truncate"><span aria-hidden="true">› </span>Jump to host, view or action</span>
        <span class="ml-auto flex shrink-0 items-center gap-0.5" aria-hidden="true">
          {#each paletteKeys as key (key)}<kbd class="kbd">{key}</kbd>{/each}
        </span>
      </button>
    {/if}
  </div>

  <div class="flex h-full items-center justify-end {IS_MAC ? 'pr-2' : ''}" data-tauri-drag-region>
    {#if onOpenShortcuts}
      <button
        type="button"
        class="btn-icon"
        onclick={onOpenShortcuts}
        title="Keyboard shortcuts (?)"
        aria-label="Keyboard shortcuts"
      >
        <Keyboard size={14} />
      </button>
    {/if}
    {#if onOpenAbout}
      <button
        type="button"
        class="btn-icon {IS_MAC ? '' : 'mr-1'}"
        onclick={onOpenAbout}
        title="About Bayesian SSH"
        aria-label="About Bayesian SSH"
      >
        <Info size={14} />
      </button>
    {/if}

    {#if !IS_MAC}
      <div class="flex h-full items-stretch">
        <button
          type="button"
          class="window-btn"
          onclick={handleWindowMinimize}
          title="Minimize"
          aria-label="Minimize"
        >
          <svg viewBox="0 0 10 10" class="size-2.5" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true">
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
          <svg viewBox="0 0 10 10" class="size-2.5" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true">
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
          <svg viewBox="0 0 10 10" class="size-2.5" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true">
            <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" />
          </svg>
        </button>
      </div>
    {/if}
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
    background: var(--color-danger-strong);
    color: var(--color-on-danger);
  }
</style>
