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
    <h3 class="settings-heading">Kerberos GSSAPI</h3>
    <p class="settings-desc">Configure Kerberos ticket expiry monitoring and automatic renewal warning thresholds</p>
  </div>

  <div class="settings-divider"></div>

  <div class="setting-row">
    <div class="setting-row-main">
      <span class="setting-title">Monitor ticket expiry</span>
      <span class="setting-meta">Track remaining ticket lifetime and warn before credentials expire</span>
    </div>
    <input
      type="checkbox"
      checked={settings.monitor_kerberos}
      onchange={(e) => {
        settings.monitor_kerberos = (e.target as HTMLInputElement).checked;
        onSave();
      }}
      class="w-[18px] h-[18px] accent-accent cursor-pointer shrink-0"
    />
  </div>

  <div class="field">
    <label for="settings-kerberos-warn" class="field-label">Warning Threshold (Minutes)</label>
    <span class="field-meta">Opens the renew ticket prompt when your ticket has less than this many minutes remaining</span>
    <input
      id="settings-kerberos-warn"
      type="number"
      min="1"
      max="1440"
      value={settings.kerberos_warn_minutes}
      onchange={(e) => {
        settings.kerberos_warn_minutes = Number((e.target as HTMLInputElement).value);
        onSave();
      }}
      class="input mt-1"
    />
  </div>
</div>
