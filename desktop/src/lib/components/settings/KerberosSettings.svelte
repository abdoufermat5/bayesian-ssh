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
    <h2 class="settings-heading">Kerberos</h2>
    <p class="settings-desc">Ticket monitoring for GSSAPI authentication.</p>
  </div>

  <div class="settings-group">
    <label class="setting-row cursor-pointer">
      <span class="setting-row-main">
        <span class="setting-title">Monitor ticket expiry</span>
        <span class="setting-meta">Track the remaining ticket lifetime and warn before it expires.</span>
      </span>
      <input
        type="checkbox"
        class="switch"
        checked={settings.monitor_kerberos}
        onchange={(e) => {
          settings.monitor_kerberos = (e.target as HTMLInputElement).checked;
          onSave();
        }}
      />
    </label>
    <div class="setting-row">
      <div class="setting-row-main">
        <label class="setting-title" for="settings-kerberos-warn">Warning threshold</label>
        <span class="setting-meta">Prompt to renew when fewer minutes than this remain.</span>
      </div>
      <div class="setting-control">
        <input
          id="settings-kerberos-warn"
          type="number"
          min="1"
          max="1440"
          disabled={!settings.monitor_kerberos}
          value={settings.kerberos_warn_minutes}
          onchange={(e) => {
            settings.kerberos_warn_minutes = Number((e.target as HTMLInputElement).value);
            onSave();
          }}
          class="input w-24 tabular-nums"
        />
        <span class="text-sm text-muted">min</span>
      </div>
    </div>
  </div>
</div>
