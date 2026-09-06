<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Link2, Minus, Server, Square, X } from "lucide-svelte";
  import { refreshWindowState } from "$lib/stores/window.svelte";

  interface Props {
    title: string;
    dockHintActive?: boolean;
    onClose?: () => void | Promise<void>;
    onDock?: () => void | Promise<void>;
  }

  let { title, dockHintActive = false, onClose, onDock }: Props = $props();

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
  class="terminal-window-bar {dockHintActive ? 'terminal-window-bar-dock' : ''}"
>
  <div class="terminal-window-title" data-tauri-drag-region>
    <span class="terminal-window-mark">
      <Server size={13} />
    </span>
    <span class="terminal-window-name">{title}</span>
    {#if dockHintActive}
      <span class="terminal-dock-hint">Drop onto main window to dock</span>
    {/if}
  </div>

  <div class="terminal-window-actions">
    {#if onDock}
      <button
        class="terminal-window-dock"
        onclick={() => onDock?.()}
        title="Dock back to main window"
      >
        <Link2 size={13} />
        <span>Dock</span>
      </button>
    {/if}
    <button
      class="terminal-window-button"
      onclick={handleWindowMinimize}
      title="Minimize"
    >
      <Minus size={13} />
    </button>
    <button
      class="terminal-window-button"
      onclick={handleWindowMaximize}
      title="Maximize/Restore"
    >
      <Square size={11} />
    </button>
    <button
      class="terminal-window-button terminal-window-button-close"
      onclick={handleWindowClose}
      title="Close"
    >
      <X size={14} />
    </button>
  </div>
</header>

<style>
  .terminal-window-bar {
    display: flex;
    height: 34px;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    overflow: hidden;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-secondary);
    user-select: none;
    transition: background-color var(--transition-duration-fast), border-color var(--transition-duration-fast);
  }

  .terminal-window-bar-dock {
    border-bottom-color: var(--color-border-hover);
    background: var(--color-surface-hover);
  }

  .terminal-window-title {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 8px;
    height: 100%;
    padding-left: 10px;
    color: var(--color-primary);
  }

  .terminal-window-mark {
    display: inline-flex;
    width: 20px;
    height: 20px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xs);
    background: var(--color-surface-input);
    color: var(--color-muted);
  }

  .terminal-window-name {
    overflow: hidden;
    color: var(--color-primary);
    font-size: 11px;
    font-weight: 800;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .terminal-dock-hint {
    flex: 0 0 auto;
    color: var(--color-accent);
    font-size: 11px;
    font-weight: 700;
    white-space: nowrap;
  }

  .terminal-window-actions {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 2px;
    height: 100%;
    padding-right: 4px;
  }

  .terminal-window-dock,
  .terminal-window-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--color-muted);
    cursor: pointer;
    transition: background-color var(--transition-duration-fast), border-color var(--transition-duration-fast),
      color var(--transition-duration-fast);
  }

  .terminal-window-dock {
    gap: 6px;
    height: 26px;
    margin-right: 4px;
    padding: 0 9px;
    border-color: color-mix(in srgb, var(--color-accent) 38%, var(--color-border));
    color: var(--color-accent);
    font-size: 11px;
    font-weight: 800;
  }

  .terminal-window-button {
    width: 38px;
    height: 28px;
  }

  .terminal-window-dock:hover,
  .terminal-window-dock:focus-visible,
  .terminal-window-button:hover,
  .terminal-window-button:focus-visible {
    border-color: var(--color-border-hover);
    background: var(--color-surface-hover);
    color: var(--color-primary);
  }

  .terminal-window-button-close:hover,
  .terminal-window-button-close:focus-visible {
    border-color: color-mix(in srgb, var(--color-error) 34%, var(--color-border));
    background: color-mix(in srgb, var(--color-error) 16%, var(--color-surface));
    color: var(--color-error);
  }
</style>
