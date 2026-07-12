<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Link2, Server } from "lucide-svelte";
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

<header class="terminal-window-titlebar" class:dock-ready={dockHintActive}>
  <div class="titlebar-left" data-tauri-drag-region>
    <Server size={14} />
    <span class="titlebar-host">{title}</span>
    {#if dockHintActive}
      <span class="dock-hint">Drop onto main window to dock</span>
    {/if}
  </div>

  <div class="titlebar-right">
    {#if onDock}
      <button class="dock-btn" onclick={() => onDock?.()} title="Dock back to main window">
        <Link2 size={13} />
        <span>Dock</span>
      </button>
    {/if}
    <button class="win-ctrl-btn" onclick={handleWindowMinimize} title="Minimize">
      <svg viewBox="0 0 10 1"><line x1="0" y1="0.5" x2="10" y2="0.5" stroke="currentColor" stroke-width="1.5" /></svg>
    </button>
    <button class="win-ctrl-btn" onclick={handleWindowMaximize} title="Maximize/Restore">
      <svg viewBox="0 0 10 10"><rect x="1" y="1" width="8" height="8" fill="none" stroke="currentColor" stroke-width="1.2" /></svg>
    </button>
    <button class="win-ctrl-btn close" onclick={handleWindowClose} title="Close">
      <svg viewBox="0 0 10 10"><path d="M1 1 L9 9 M9 1 L1 9" fill="none" stroke="currentColor" stroke-width="1.2" /></svg>
    </button>
  </div>
</header>

<style>
  .terminal-window-titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    padding: 0 12px;
    background: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-color);
    user-select: none;
    flex-shrink: 0;
    transition: background 0.15s ease, border-color 0.15s ease;
  }

  .terminal-window-titlebar.dock-ready {
    background: rgba(0, 240, 255, 0.08);
    border-bottom-color: rgba(0, 240, 255, 0.35);
  }

  .titlebar-left {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    height: 100%;
    color: var(--accent-cyan);
  }

  .titlebar-host {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dock-hint {
    margin-left: 8px;
    font-size: 11px;
    font-weight: 500;
    color: var(--accent-cyan);
    white-space: nowrap;
  }

  .titlebar-right {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    gap: 4px;
  }

  .dock-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    margin-right: 4px;
    padding: 0 10px;
    border-radius: 6px;
    border: 1px solid rgba(0, 240, 255, 0.25);
    background: rgba(0, 240, 255, 0.06);
    color: var(--accent-cyan);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }

  .dock-btn:hover {
    background: rgba(0, 240, 255, 0.12);
  }

  .win-ctrl-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 46px;
    height: 32px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .win-ctrl-btn svg {
    width: 10px;
    height: 10px;
  }

  .win-ctrl-btn:hover {
    background: rgba(255, 255, 255, 0.06);
    color: var(--text-primary);
  }

  .win-ctrl-btn.close:hover {
    background: #ef4444;
    color: white;
  }
</style>
