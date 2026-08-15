<script lang="ts">
  import type { DesktopSettings } from "$lib/types";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="flex flex-col gap-6 max-w-2xl">
  <div>
    <h3 class="text-base font-semibold text-primary m-0">Kerberos GSSAPI</h3>
    <p class="text-xs text-muted mt-1">Configure Kerberos ticket expiry monitoring and automatic renewal warning thresholds</p>
  </div>

  <div class="h-px bg-border/50"></div>

  <div class="flex items-center justify-between gap-4 py-1">
    <div class="flex flex-col gap-0.5">
      <span class="text-xs font-semibold text-secondary">Monitor ticket expiry</span>
      <span class="text-[11px] text-muted leading-snug">Track remaining ticket lifetime and warn before credentials expire</span>
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

  <div class="flex flex-col gap-1.5">
    <label for="settings-kerberos-warn" class="text-xs font-semibold text-secondary">Warning Threshold (Minutes)</label>
    <span class="text-[11px] text-muted">Opens the renew ticket prompt when your ticket has less than this many minutes remaining</span>
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
      class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
    />
  </div>
</div>
