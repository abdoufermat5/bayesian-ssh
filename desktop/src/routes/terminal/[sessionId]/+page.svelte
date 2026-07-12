<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { page } from "$app/state";
  import TerminalWindowTitleBar from "$lib/components/TerminalWindowTitleBar.svelte";
  import {
    dockPopoutToMain,
    initPopoutTerminal,
    type PopoutTerminalHandle,
  } from "$lib/stores/popout-terminal";

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
        unlistenDocked?.();
        void handle?.shutdown({ closeWindow: false });
      }
    };
  });
</script>

<div class="terminal-window-root">
  <TerminalWindowTitleBar
    title={connectionName}
    onDock={dockToMain}
    onClose={() => getCurrentWindow().close()}
  />

  {#if loadError}
    <div class="terminal-window-error">
      <p>{loadError}</p>
    </div>
  {:else}
    <div class="terminal-window-body">
      <div id="terminal-popout-root" class="terminal-popout-target"></div>
    </div>
  {/if}
</div>

<style>
  :global(html),
  :global(body),
  :global(#app-shell) {
    height: 100%;
    margin: 0;
  }

  .terminal-window-root {
    height: 100dvh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-terminal);
  }

  .terminal-window-body {
    flex: 1;
    min-height: 0;
    padding: 4px;
    box-sizing: border-box;
  }

  .terminal-popout-target {
    width: 100%;
    height: 100%;
  }

  .terminal-window-error {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
