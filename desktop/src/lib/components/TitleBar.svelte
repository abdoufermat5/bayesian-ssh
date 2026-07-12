<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Server } from "lucide-svelte";
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
  class="custom-titlebar"
  style="display: flex; align-items: center; justify-content: space-between; padding: 0 12px; height: 32px; background-color: var(--bg-sidebar); border-bottom: 1px solid var(--border-color); user-select: none;"
>
  <div
    class="titlebar-left"
    data-tauri-drag-region
    style="display: flex; align-items: center; flex: 1; height: 100%; cursor: default;"
  >
    <div
      class="app-logo"
      style="display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; margin-right: 8px; pointer-events: none;"
    >
      <svg
        viewBox="0 0 1024 1024"
        style="width: 16px; height: 16px; fill: none; stroke: var(--accent-cyan); stroke-width: 80;"
      >
        <path d="M100 800 C300 800, 400 200, 512 200 C624 200, 724 800, 924 800" />
        <line x1="512" y1="200" x2="512" y2="800" stroke="var(--accent-pink)" />
      </svg>
    </div>
    <span
      class="titlebar-app-name"
      style="font-size: 12px; font-weight: 500; color: var(--text-primary); user-select: none; pointer-events: none;"
      >Bayesian SSH</span
    >
  </div>

  <div
    class="titlebar-center"
    data-tauri-drag-region
    style="display: flex; justify-content: center; align-items: center; flex: 1; height: 100%; cursor: default;"
  >
    <div
      class="titlebar-search-bar"
      style="background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 4px; padding: 4px 16px; font-size: 11px; color: var(--text-secondary); display: flex; align-items: center; gap: 6px; user-select: none; pointer-events: none;"
    >
      <Server size={10} style="color: var(--accent-cyan);" />
      <span
        >Bayesian SSH &mdash; Profile: <strong style="color: var(--text-primary);">{activeEnv}</strong></span
      >
    </div>
  </div>

  <div
    class="titlebar-right"
    style="display: flex; align-items: center; justify-content: flex-end; flex: 1; height: 100%;"
  >
    <button
      class="win-ctrl-btn minimize"
      onclick={handleWindowMinimize}
      title="Minimize"
      style="position: relative; z-index: 9999; display: flex; align-items: center; justify-content: center; width: 46px; height: 32px; background: none; border: none; color: var(--text-secondary); cursor: pointer;"
    >
      <svg viewBox="0 0 10 1" style="width: 10px; height: 1px; fill: none; stroke: currentColor; stroke-width: 1.5;"
        ><line x1="0" y1="0.5" x2="10" y2="0.5" /></svg
      >
    </button>
    <button
      class="win-ctrl-btn maximize"
      onclick={handleWindowMaximize}
      title="Maximize/Restore"
      style="position: relative; z-index: 9999; display: flex; align-items: center; justify-content: center; width: 46px; height: 32px; background: none; border: none; color: var(--text-secondary); cursor: pointer;"
    >
      <svg viewBox="0 0 10 10" style="width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1.2;"
        ><rect x="1" y="1" width="8" height="8" /></svg
      >
    </button>
    <button
      class="win-ctrl-btn close"
      onclick={handleWindowClose}
      title="Close"
      style="position: relative; z-index: 9999; display: flex; align-items: center; justify-content: center; width: 46px; height: 32px; background: none; border: none; color: var(--text-secondary); cursor: pointer;"
    >
      <svg viewBox="0 0 10 10" style="width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1.2;"
        ><path d="M1 1 L9 9 M9 1 L1 9" /></svg
      >
    </button>
  </div>
</header>
