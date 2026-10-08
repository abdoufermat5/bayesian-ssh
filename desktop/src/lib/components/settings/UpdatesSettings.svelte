<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { CheckCircle2, RefreshCw, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import type { UpdateChannel, UpdateInfo } from "$lib/types";

  let version = $state("");
  // undefined until update_managed_by answers, so no channel UI flashes first.
  let channel = $state<UpdateChannel | null | undefined>(undefined);
  let checking = $state(false);
  let installing = $state(false);
  let upToDate = $state(false);
  let update = $state<UpdateInfo | null>(null);
  let updateError = $state("");
  let confirmOpen = $state(false);
  let activeSessions = $state(0);

  onMount(() => {
    invoke<string>("get_app_version")
      .then((v) => (version = v))
      .catch(() => (version = "unknown"));
    invoke<UpdateChannel | null>("update_managed_by")
      .then((c) => (channel = c))
      .catch(() => (channel = null));
  });

  async function checkForUpdates() {
    if (checking || channel !== null) return;
    checking = true;
    updateError = "";
    upToDate = false;
    update = null;
    try {
      const info = await invoke<UpdateInfo | null>("check_update");
      if (info) update = info;
      else upToDate = true;
    } catch (err) {
      updateError = String(err);
    } finally {
      checking = false;
    }
  }

  /** Ask the backend how many sessions would be closed, then confirm. */
  async function requestInstall() {
    if (!update || installing) return;
    try {
      activeSessions = await invoke<number>("count_active_sessions");
    } catch {
      activeSessions = 0;
    }
    confirmOpen = true;
  }

  async function confirmInstall() {
    const info = update;
    if (!info || installing) return;
    installing = true;
    try {
      await invoke<void>("install_update", { version: info.version });
      confirmOpen = false;
    } catch (err) {
      notify(`Update failed: ${err}`, "error");
    } finally {
      installing = false;
    }
  }
</script>

<div class="settings-page">
  <div>
    <h2 class="settings-heading">Updates</h2>
    <p class="settings-desc">Version and update channel for this installation.</p>
  </div>

  <section>
    <h3 class="settings-group-title">Application</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Current version</span>
          <span class="setting-meta tabular-nums">
            {#if version}{version}{:else}Checking&hellip;{/if}
          </span>
        </div>
      </div>

      {#if channel === "snap"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Managed by the Snap Store</span>
            <span class="setting-meta">
              This installation is updated by the Snap Store; the in-app updater is disabled.
            </span>
          </div>
          <code class="system-value">sudo snap refresh bayesian-ssh</code>
        </div>
      {:else if channel === "manual"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Updated manually</span>
            <span class="setting-meta">
              This installation updates the way it was installed: run install.sh, use your package
              manager, or rebuild from source.
            </span>
          </div>
        </div>
      {:else if channel === null}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Software updates</span>
            <span class="setting-meta">Check GitHub releases for a newer signed build.</span>
          </div>
          <div class="setting-control">
            <button
              type="button"
              class="btn btn-secondary"
              disabled={checking}
              onclick={checkForUpdates}
            >
              <RefreshCw size={14} class={checking ? "animate-spin" : ""} />
              {checking ? "Checking…" : "Check for updates"}
            </button>
          </div>
        </div>

        {#if update}
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Update available: {update.version}</span>
              <span class="setting-meta">Currently running {update.currentVersion}.</span>
            </div>
            <div class="setting-control">
              <button
                type="button"
                class="btn btn-primary"
                disabled={installing}
                onclick={requestInstall}
              >
                Install and restart
              </button>
            </div>
          </div>
          {#if update.notes}
            <div class="setting-row">
              <pre class="code-block m-0 w-full whitespace-pre-wrap">{update.notes}</pre>
            </div>
          {/if}
        {:else if upToDate}
          <div class="setting-row">
            <span class="setting-title inline-flex items-center gap-2">
              <CheckCircle2 size={14} class="text-accent" />
              You're on the latest version.
            </span>
          </div>
        {:else if updateError}
          <div class="setting-row">
            <span class="text-xs text-danger">Could not check for updates: {updateError}</span>
          </div>
        {/if}
      {/if}
    </div>
  </section>
</div>

{#if confirmOpen}
  <ModalShell open={confirmOpen} title="Install update" onClose={() => (confirmOpen = false)} width="sm">
    <div class="modal-header">
      <h2 class="modal-title">Install Bayesian SSH {update?.version}?</h2>
      <button type="button" class="modal-close" onclick={() => (confirmOpen = false)} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-2.5">
      <p class="m-0 text-sm leading-relaxed text-secondary">
        The update is downloaded, verified and installed, then the app restarts.
      </p>
      {#if activeSessions > 0}
        <p class="m-0 text-sm leading-relaxed text-secondary">
          <span class="font-medium text-primary">{activeSessions}
            {activeSessions === 1 ? "session" : "sessions"}</span>
          will be closed.
        </p>
      {/if}
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={() => (confirmOpen = false)} data-autofocus>
        Cancel
      </button>
      <button type="button" class="btn btn-primary" disabled={installing} onclick={confirmInstall}>
        Install and restart
      </button>
    </div>
  </ModalShell>
{/if}
