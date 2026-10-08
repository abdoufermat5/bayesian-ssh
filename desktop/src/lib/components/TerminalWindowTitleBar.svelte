<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Link2 } from "lucide-svelte";

  interface Props {
    title: string;
    dockHintActive?: boolean;
    /** Session state shown as a dot before the title. */
    status?: "connected" | "exited";
    onClose?: () => void | Promise<void>;
    onDock?: () => void | Promise<void>;
  }

  let { title, dockHintActive = false, status = "connected", onClose, onDock }: Props = $props();

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
      if (onClose) {
        await onClose();
        return;
      }
      await appWindow.close();
    } catch (e) {
      console.error(e);
    }
  }
</script>

<header
  class="flex h-9 shrink-0 select-none items-center gap-3 border-b border-border pl-3 transition-colors duration-fast
    {dockHintActive ? 'bg-surface-hover' : 'bg-chrome'}"
  data-tauri-drag-region
>
  <div class="flex min-w-0 flex-1 items-center gap-2" data-tauri-drag-region>
    <span
      class="status-dot status-dot-sm {status === 'connected' ? 'status-dot-success' : 'status-dot-offline'}"
      title={status === "connected" ? "Connected" : "Disconnected"}
      aria-hidden="true"
    ></span>
    <span class="truncate text-sm font-medium text-primary" data-tauri-drag-region>{title}</span>
    {#if dockHintActive}
      <span class="shrink-0 text-xs text-accent">Drop onto the main window to dock</span>
    {/if}
  </div>

  <div class="flex h-full shrink-0 items-center">
    {#if onDock}
      <button
        type="button"
        class="btn btn-ghost btn-sm mr-1"
        title="Move this session back into the main window"
        onclick={() => onDock?.()}
      >
        <Link2 size={14} />
        Dock
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
        title="Close session"
        aria-label="Close session"
      >
        <svg viewBox="0 0 10 10" class="size-2.5" fill="none" stroke="currentColor" stroke-width="1.2">
          <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" />
        </svg>
      </button>
    </div>
  </div>
</header>

<style>
  /* Mirrors the main window's TitleBar controls. */
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
    background: var(--color-error);
    color: white;
  }
</style>
