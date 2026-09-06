<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/state";
  import TerminalWindowTitleBar from "$lib/components/TerminalWindowTitleBar.svelte";
  import {
    dockPopoutToMain,
    initPopoutTerminal,
    type PopoutTerminalHandle,
  } from "$lib/stores/popout-terminal";
  import { applyTheme } from "$lib/utils/theme";

  let connectionName = $state("Terminal");
  let loadError = $state<string | null>(null);

  const sessionId = $derived(page.params.sessionId ?? "");

  let closing = false;
  let unlistenClose: (() => void) | undefined;
  let unlistenDocked: (() => void) | undefined;
  let handle: PopoutTerminalHandle | null = null;
  let initPromise: Promise<void> | null = null;
  const win = getCurrentWindow();

  async function shutdownAndDestroy() {
    if (closing) return;
    closing = true;
    unlistenClose?.();
    unlistenClose = undefined;
    unlistenDocked?.();
    unlistenDocked = undefined;

    if (initPromise) {
      await initPromise.catch(() => {});
    }

    if (handle) {
      await handle.shutdown({ closeWindow: false });
    }

    await win.destroy();
  }

  async function dockToMain() {
    if (closing || !handle || !sessionId) return;
    closing = true;
    unlistenClose?.();
    unlistenClose = undefined;
    unlistenDocked?.();
    unlistenDocked = undefined;

    try {
      await invoke("seal_session_ui", { sessionId });
    } catch {
      // Continue docking even if seal fails.
    }

    handle.releaseUi();
    handle = null;

    try {
      // Rust destroys this window after docking; no local destroy needed.
      await dockPopoutToMain(sessionId);
    } catch (e: unknown) {
      loadError = String(e);
      closing = false;
    }
  }

  onMount(() => {
    let cancelled = false;

    // Load and apply the theme for the popout window
    invoke("load_desktop_settings").then((settings: any) => {
      if (settings && settings.theme) {
        applyTheme(settings.theme);
      }
    });

    void win
      .onCloseRequested((event) => {
        event.preventDefault();
        void shutdownAndDestroy();
      })
      .then((unlisten) => {
        if (closing) {
          unlisten();
          return;
        }
        unlistenClose = unlisten;
      })
      .catch(() => {});

    initPromise = (async () => {
      if (!sessionId) {
        loadError = "Missing session id.";
        return;
      }

      unlistenDocked = await listen("session-docked", (event) => {
        const info = event.payload as { session_id?: string };
        if (info.session_id !== sessionId || closing) return;
        closing = true;
        handle?.releaseUi();
        handle = null;
        unlistenClose?.();
        unlistenClose = undefined;
        unlistenDocked?.();
        unlistenDocked = undefined;
      });

      try {
        handle = await initPopoutTerminal(sessionId);
        if (cancelled || closing) {
          handle.releaseUi();
          return;
        }
        connectionName = handle.connectionName;
      } catch (e: unknown) {
        loadError = String(e);
      }
    })();

    return () => {
      cancelled = true;
      if (!closing) {
        unlistenClose?.();
        void handle?.shutdown({ closeWindow: false });
      }
    };
  });
</script>

<div class="terminal-popout-shell">
  <TerminalWindowTitleBar
    title={connectionName}
    onDock={dockToMain}
    onClose={() => getCurrentWindow().close()}
  />

  {#if loadError}
    <div class="terminal-popout-error">
      <p>{loadError}</p>
    </div>
  {:else}
    <div class="terminal-popout-body">
      <div id="terminal-popout-root" class="terminal-popout-target"></div>
    </div>
  {/if}
</div>

<style>
  .terminal-popout-shell {
    display: flex;
    height: 100dvh;
    flex-direction: column;
    overflow: hidden;
    background: var(--color-surface-terminal);
  }

  .terminal-popout-body {
    box-sizing: border-box;
    flex: 1;
    min-height: 0;
    padding: 4px;
    background: var(--color-surface-terminal);
  }

  .terminal-popout-error {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    padding: 24px;
    color: var(--color-muted);
    font-size: 12px;
    text-align: center;
  }
</style>
