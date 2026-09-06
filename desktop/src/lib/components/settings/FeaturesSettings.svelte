<script lang="ts">
  import { FolderTree, Network } from "lucide-svelte";
  import type { DesktopSettings } from "$lib/types";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="settings-page">
  <div>
    <h3 class="settings-heading">Features</h3>
    <p class="settings-desc">Enable or disable optional sections of the application. Disabled sections are hidden from the sidebar.</p>
  </div>

  <div class="settings-divider"></div>

  <div class="flex flex-col gap-3">
    <div class="setting-row">
      <div class="flex items-center gap-3">
        <div class="icon-tile">
          <FolderTree size={16} />
        </div>
        <div class="setting-row-main">
          <p class="setting-title m-0">SFTP File Browser</p>
          <p class="setting-meta m-0">Browse and manage remote files over SSH using the built-in SFTP browser</p>
        </div>
      </div>
      <button
        type="button"
        role="switch"
        aria-label="Toggle SFTP File Browser"
        aria-checked={settings.enable_sftp !== false}
        onclick={() => { settings.enable_sftp = settings.enable_sftp === false ? true : false; onSave(); }}
        class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent/30
          {settings.enable_sftp !== false ? 'bg-accent' : 'bg-surface-input border border-border'}"
      >
        <span
          class="pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow transition duration-200 ease-in-out
            {settings.enable_sftp !== false ? 'translate-x-5' : 'translate-x-0'}"
        ></span>
      </button>
    </div>

    <div class="setting-row">
      <div class="flex items-center gap-3">
        <div class="icon-tile">
          <Network size={16} />
        </div>
        <div class="setting-row-main">
          <p class="setting-title m-0">Tunnel Studio</p>
          <p class="setting-meta m-0">Manage SSH local port-forward tunnels and SOCKS5 dynamic proxies</p>
        </div>
      </div>
      <button
        type="button"
        role="switch"
        aria-label="Toggle Tunnel Studio"
        aria-checked={settings.enable_tunneling !== false}
        onclick={() => { settings.enable_tunneling = settings.enable_tunneling === false ? true : false; onSave(); }}
        class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent/30
          {settings.enable_tunneling !== false ? 'bg-accent' : 'bg-surface-input border border-border'}"
      >
        <span
          class="pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow transition duration-200 ease-in-out
            {settings.enable_tunneling !== false ? 'translate-x-5' : 'translate-x-0'}"
        ></span>
      </button>
    </div>
  </div>

  <p class="field-meta">
    Changes take effect immediately; no restart required.
  </p>
</div>
