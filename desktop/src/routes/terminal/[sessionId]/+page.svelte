<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/state";
  import TerminalWindowTitleBar from "$lib/components/TerminalWindowTitleBar.svelte";
  import {
    checkPopoutMainOverlap,
    dockPopoutToMain,
    initPopoutTerminal,
    type PopoutTerminalHandle,
  } from "$lib/stores/popout-terminal";
  import { applyTheme } from "$lib/utils/theme";

  let connectionName = $state("Terminal");
  let loadError = $state<string | null>(null);
  let dockHintActive = $state(false);

  const sessionId = $derived(page.params.sessionId ?? "");

  let closing = false;
  let unlistenClose: (() => void) | undefined;
  let unlistenDocked: (() => void) | undefined;
  let unlistenMoved: (() => void) | undefined;
  let handle: PopoutTerminalHandle | null = null;
  let initPromise: Promise<void> | null = null;
  let overlapCheckTimer: ReturnType<typeof setTimeout> | undefined;
  let overlapCheckInFlight = false;
  const win = getCurrentWindow();

  async function checkDragDock() {
    if (closing || !handle || !sessionId || overlapCheckInFlight) return;

    overlapCheckInFlight = true;
    try {
      const overlap = await checkPopoutMainOverlap(win.label);
      if (closing) return;

      dockHintActive = overlap.overlaps && !overlap.should_dock;

      if (overlap.should_dock) {
        dockHintActive = false;
        await dockToMain();
      }
    } catch {
      dockHintActive = false;
    } finally {
      overlapCheckInFlight = false;
    }
  }

  function scheduleDragDockCheck() {
    if (closing || overlapCheckTimer) return;
    overlapCheckTimer = setTimeout(() => {
      overlapCheckTimer = undefined;
      void checkDragDock();
    }, 80);
  }

  async function shutdownAndDestroy() {
    if (closing) return;
    closing = true;
    unlistenClose?.();
    unlistenClose = undefined;
    unlistenDocked?.();
    unlistenDocked = undefined;
    unlistenMoved?.();
    unlistenMoved = undefined;
    if (overlapCheckTimer) {
      clearTimeout(overlapCheckTimer);
      overlapCheckTimer = undefined;
    }

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
    unlistenMoved?.();
    unlistenMoved = undefined;
    if (overlapCheckTimer) {
      clearTimeout(overlapCheckTimer);
      overlapCheckTimer = undefined;
    }

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

    void win
      .onMoved(() => {
        scheduleDragDockCheck();
      })
      .then((unlisten) => {
        if (closing) {
          unlisten();
          return;
        }
        unlistenMoved = unlisten;
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
        unlistenMoved?.();
        unlistenMoved = undefined;
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
        unlistenMoved?.();
        if (overlapCheckTimer) {
          clearTimeout(overlapCheckTimer);
        }
        void handle?.shutdown({ closeWindow: false });
      }
    };
  });
</script>

<div
  class="h-[100dvh] flex flex-col overflow-hidden bg-surface-terminal transition-all duration-150
    {dockHintActive ? 'shadow-[inset_0_0_0_2px_rgba(59,130,246,0.45)]' : ''}"
>
  <TerminalWindowTitleBar
    title={connectionName}
    dockHintActive={dockHintActive}
    onDock={dockToMain}
    onClose={() => getCurrentWindow().close()}
  />

  {#if loadError}
    <div class="flex-1 flex items-center justify-center text-muted text-xs">
      <p>{loadError}</p>
    </div>
  {:else}
    <div class="flex-1 min-h-0 p-1 box-border">
      <div id="terminal-popout-root" class="w-full h-full"></div>
    </div>
  {/if}
</div>
