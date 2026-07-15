<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { refreshWindowState } from "$lib/stores/window.svelte";

  interface Props {
    activeEnv: string;
  }

  let { activeEnv }: Props = $props();

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
  class="flex shrink-0 items-center justify-between h-[var(--titlebar-h)] bg-surface border-b border-border px-3 select-none z-50"
>
  <div
    class="flex items-center flex-1 h-full cursor-default gap-2"
    data-tauri-drag-region
  >
    <div class="flex items-center justify-center w-6 h-6 pointer-events-none shrink-0">
      <img src="/favicon.png" alt="" class="w-4 h-4 rounded-sm" draggable="false" />
    </div>
    <span class="text-xs font-medium text-secondary select-none pointer-events-none">
      Bayesian SSH
    </span>
  </div>

  <div
    class="flex justify-center items-center flex-1 h-full cursor-default"
    data-tauri-drag-region
  >
    <div
      class="bg-surface-raised border border-border rounded-md px-3.5 py-0.5 text-[11px] text-secondary flex items-center gap-1.5 select-none pointer-events-none"
    >
      <span>Profile: <strong class="text-primary font-semibold">{activeEnv}</strong></span>
    </div>
  </div>

  <div class="flex items-center justify-end flex-1 h-full">
    <button
      class="flex items-center justify-center w-[46px] h-[var(--titlebar-h)] bg-transparent border-none text-muted cursor-pointer transition-colors duration-100 hover:bg-white/5 hover:text-primary"
      onclick={handleWindowMinimize}
      title="Minimize"
    >
      <svg viewBox="0 0 10 1" class="w-2.5 h-px fill-none stroke-current" style="stroke-width: 1.5;">
        <line x1="0" y1="0.5" x2="10" y2="0.5" />
      </svg>
    </button>
    <button
      class="flex items-center justify-center w-[46px] h-[var(--titlebar-h)] bg-transparent border-none text-muted cursor-pointer transition-colors duration-100 hover:bg-white/5 hover:text-primary"
      onclick={handleWindowMaximize}
      title="Maximize/Restore"
    >
      <svg viewBox="0 0 10 10" class="w-2.5 h-2.5 fill-none stroke-current" style="stroke-width: 1.2;">
        <rect x="1" y="1" width="8" height="8" />
      </svg>
    </button>
    <button
      class="flex items-center justify-center w-[46px] h-[var(--titlebar-h)] bg-transparent border-none text-muted cursor-pointer transition-colors duration-100 hover:bg-red-500 hover:!text-white"
      onclick={handleWindowClose}
      title="Close"
    >
      <svg viewBox="0 0 10 10" class="w-2.5 h-2.5 fill-none stroke-current" style="stroke-width: 1.2;">
        <path d="M1 1 L9 9 M9 1 L1 9" />
      </svg>
    </button>
  </div>
</header>
