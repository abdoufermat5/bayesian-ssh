<script lang="ts">
  import type { DesktopSettings } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import {
    SYSTEM_TIMEZONE,
    getSupportedTimezones,
    getSystemTimezone,
  } from "$lib/utils/timezone";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
    onThemeChange: (theme: string) => void;
  }

  let { settings = $bindable(), onSave, onThemeChange }: Props = $props();

  const systemTimezone = getSystemTimezone();
  const timezoneOptions = getSupportedTimezones();
  let timezoneFilter = $state("");

  const filteredTimezoneOptions = $derived.by(() => {
    const query = timezoneFilter.trim().toLowerCase();
    const selected = settings.timezone;
    const base = !query
      ? timezoneOptions
      : timezoneOptions.filter((tz) => tz.toLowerCase().includes(query));

    if (selected && selected !== SYSTEM_TIMEZONE && !base.includes(selected)) {
      return [selected, ...base];
    }

    return base;
  });
</script>

<div class="flex flex-col gap-6 max-w-2xl">
  <div>
    <h3 class="text-base font-semibold text-primary m-0">Appearance & Locale</h3>
    <p class="text-xs text-muted mt-1">Configure active UI color theme and application timezone</p>
  </div>

  <div class="h-px bg-border/50"></div>

  <div class="flex flex-col gap-1.5">
    <label id="settings-theme-label" for="settings-theme" class="text-xs font-semibold text-secondary">Active UI Theme</label>
    <span class="text-[11px] text-muted">Choose your preferred visual style and colors</span>
    <CustomSelect
      id="settings-theme"
      options={[
        { value: "zinc", label: "Slate Minimalist (Zinc)" },
        { value: "cyberpunk", label: "Cyberpunk Cyan (Neon Glow)" },
        { value: "oled", label: "OLED Pitch Black" },
        { value: "slate", label: "Sleek Navy (Slate)" }
      ]}
      value={settings.theme}
      onChange={(val) => onThemeChange(val)}
    />
  </div>

  <div class="flex flex-col gap-2">
    <label for="settings-timezone" class="text-xs font-semibold text-secondary">Application Timezone</label>
    <span class="text-[11px] text-muted">Dates and times across logs and metrics are displayed in this timezone</span>
    <div class="flex flex-col gap-1.5 mt-1">
      <input
        id="settings-timezone-filter"
        type="text"
        placeholder="Filter timezones..."
        bind:value={timezoneFilter}
        class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
      />
      <CustomSelect
        id="settings-timezone"
        options={[
          { value: SYSTEM_TIMEZONE, label: `System default (${systemTimezone})` },
          ...filteredTimezoneOptions.map((tz) => ({ value: tz, label: tz }))
        ]}
        value={settings.timezone}
        onChange={(val) => {
          settings.timezone = val;
          onSave();
        }}
      />
    </div>
  </div>
</div>
