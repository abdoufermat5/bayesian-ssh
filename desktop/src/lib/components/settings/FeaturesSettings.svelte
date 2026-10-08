<script lang="ts">
  import type { DesktopSettings } from "$lib/types";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="settings-page">
  <div>
    <h2 class="settings-heading">Features</h2>
    <p class="settings-desc">Turn optional sections on or off. Disabled sections are hidden from the sidebar.</p>
  </div>

  <div class="settings-group">
    <label class="setting-row cursor-pointer">
      <span class="setting-row-main">
        <span class="setting-title">SFTP file browser</span>
        <span class="setting-meta">Browse and transfer remote files over SSH.</span>
      </span>
      <input
        type="checkbox"
        class="switch"
        checked={settings.enable_sftp !== false}
        onchange={(e) => {
          settings.enable_sftp = (e.target as HTMLInputElement).checked;
          onSave();
        }}
      />
    </label>
    <label class="setting-row cursor-pointer">
      <span class="setting-row-main">
        <span class="setting-title">Tunnels</span>
        <span class="setting-meta">Local port forwards and SOCKS5 dynamic proxies.</span>
      </span>
      <input
        type="checkbox"
        class="switch"
        checked={settings.enable_tunneling !== false}
        onchange={(e) => {
          settings.enable_tunneling = (e.target as HTMLInputElement).checked;
          onSave();
        }}
      />
    </label>
  </div>

  <p class="-mt-3 text-xs text-muted">Changes apply immediately.</p>
</div>
