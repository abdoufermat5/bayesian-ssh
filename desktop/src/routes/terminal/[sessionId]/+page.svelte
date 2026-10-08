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
  let status = $state<"connected" | "exited">("connected");

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
    invoke<{ theme?: string } | null>("load_desktop_settings")
      .then((settings) => {
        if (settings?.theme) {
          applyTheme(settings.theme);
        }
      })
      .catch(() => {
        // Keep the default theme if settings are unavailable.
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
        handle = await initPopoutTerminal(sessionId, {
          onExit: () => (status = "exited"),
        });
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
      unlistenDocked?.();
      unlistenDocked = undefined;
      if (!closing) {
        unlistenClose?.();
        void handle?.shutdown({ closeWindow: false });
      }
    };
  });
</script>

<div class="flex h-dvh flex-col overflow-hidden bg-surface-terminal">
  <TerminalWindowTitleBar
    title={connectionName}
    status={loadError ? "exited" : status}
    onDock={loadError ? undefined : dockToMain}
    onClose={() => getCurrentWindow().close()}
  />

  {#if loadError}
    <div class="empty-state flex-1 bg-surface">
      <p class="empty-state-title">Couldn't open this session</p>
      <p class="empty-state-desc">{loadError}</p>
      <div class="empty-state-action">
        <button type="button" class="btn btn-secondary" onclick={() => getCurrentWindow().close()}>
          Close window
        </button>
      </div>
    </div>
  {:else}
    <div class="box-border min-h-0 flex-1 bg-surface-terminal p-2">
      <div id="terminal-popout-root" class="h-full w-full"></div>
    </div>
  {/if}
</div>
