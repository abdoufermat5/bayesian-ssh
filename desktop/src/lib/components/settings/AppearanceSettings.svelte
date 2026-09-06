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

<div class="settings-page">
  <div>
    <h3 class="settings-heading">Appearance & Locale</h3>
    <p class="settings-desc">Configure active UI color theme and application timezone</p>
  </div>

  <div class="settings-divider"></div>

  <div class="field">
    <label id="settings-theme-label" for="settings-theme" class="field-label">Active UI Theme</label>
    <span class="field-meta">Choose your preferred visual style and colors</span>
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

  <div class="field">
    <label for="settings-timezone" class="field-label">Application Timezone</label>
    <span class="field-meta">Dates and times across logs and metrics are displayed in this timezone</span>
    <div class="flex flex-col gap-1.5 mt-1">
      <input
        id="settings-timezone-filter"
        type="text"
        placeholder="Filter timezones..."
        bind:value={timezoneFilter}
        class="input"
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
