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

<header
  class="flex items-center justify-between h-[32px] px-3 bg-surface border-b border-border select-none shrink-0 transition-all duration-150
    {dockHintActive ? 'bg-accent/8 border-b-accent/35' : ''}"
>
  <div class="flex items-center gap-2 flex-1 min-w-0 h-full text-accent" data-tauri-drag-region>
    <Server size={14} />
    <span class="text-xs font-semibold text-primary overflow-hidden text-ellipsis whitespace-nowrap">{title}</span>
    {#if dockHintActive}
      <span class="ml-2 text-[11px] font-medium text-accent whitespace-nowrap">Drop onto main window to dock</span>
    {/if}
  </div>

  <div class="flex items-center shrink-0 gap-1">
    {#if onDock}
      <button
        class="inline-flex items-center gap-1.5 h-6 mr-1 px-2.5 rounded-md border border-accent/25 bg-accent/6 text-accent text-[11px] font-semibold cursor-pointer transition-colors hover:bg-accent/12"
        onclick={() => onDock?.()}
        title="Dock back to main window"
      >
        <Link2 size={13} />
        <span>Dock</span>
      </button>
    {/if}
    <button
      class="flex items-center justify-center w-[46px] h-8 bg-transparent border-none text-muted cursor-pointer transition-colors duration-100 hover:bg-white/5 hover:text-primary"
      onclick={handleWindowMinimize}
      title="Minimize"
    >
      <svg viewBox="0 0 10 1" class="w-2.5 h-px fill-none stroke-current" style="stroke-width: 1.5;"><line x1="0" y1="0.5" x2="10" y2="0.5" /></svg>
    </button>
    <button
      class="flex items-center justify-center w-[46px] h-8 bg-transparent border-none text-muted cursor-pointer transition-colors duration-100 hover:bg-white/5 hover:text-primary"
      onclick={handleWindowMaximize}
      title="Maximize/Restore"
    >
      <svg viewBox="0 0 10 10" class="w-2.5 h-2.5 fill-none stroke-current" style="stroke-width: 1.2;"><rect x="1" y="1" width="8" height="8" /></svg>
    </button>
    <button
      class="flex items-center justify-center w-[46px] h-8 bg-transparent border-none text-muted cursor-pointer transition-colors duration-100 hover:bg-red-500 hover:text-white"
      onclick={handleWindowClose}
      title="Close"
    >
      <svg viewBox="0 0 10 10" class="w-2.5 h-2.5 fill-none stroke-current" style="stroke-width: 1.2;"><path d="M1 1 L9 9 M9 1 L1 9" /></svg>
    </button>
  </div>
</header>
