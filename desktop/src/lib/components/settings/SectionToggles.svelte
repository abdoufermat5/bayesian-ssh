<script lang="ts">
  import type { DesktopSettings } from "$lib/types";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="settings-page">
  <section>
    <h2 class="settings-group-title">Optional sections</h2>
    <p class="setting-meta -mt-1 mb-2">When off, Files is hidden from the sidebar.</p>
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
    </div>
  </section>
</div>
