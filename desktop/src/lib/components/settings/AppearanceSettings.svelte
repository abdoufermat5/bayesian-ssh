<script lang="ts">
  import { Check } from "lucide-svelte";
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

  // Swatch colors mirror the per-theme tokens in styles/tokens.css. They are a
  // preview of themes other than the active one, so they can't use utilities.
  const themes = [
    { id: "zinc", name: "Graphite", chrome: "#0b0c0e", surface: "#101114", panel: "#15161a", border: "#222329", text: "#ededf0", muted: "#6e7079", accent: "#7b88ff" },
    { id: "cyberpunk", name: "Midnight", chrome: "#070b14", surface: "#0a101c", panel: "#0f1725", border: "#1a2538", text: "#e6f1ff", muted: "#5b6f8f", accent: "#4cc9f0" },
    { id: "oled", name: "OLED black", chrome: "#000000", surface: "#000000", panel: "#0a0a0b", border: "#1c1c1f", text: "#f4f4f5", muted: "#66666e", accent: "#e8e8ea" },
    { id: "slate", name: "Slate", chrome: "#0c111a", surface: "#0f1520", panel: "#141b28", border: "#1f2939", text: "#eef2f7", muted: "#64748b", accent: "#38bdf8" },
  ];

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

  const timezoneSelectOptions = $derived([
    { value: SYSTEM_TIMEZONE, label: `System (${systemTimezone})` },
    ...filteredTimezoneOptions.map((tz) => ({ value: tz, label: tz })),
  ]);
</script>

<div class="settings-page">
  <div>
    <h2 class="settings-heading">Appearance</h2>
    <p class="settings-desc">Color theme and how dates are displayed.</p>
  </div>

  <section>
    <h3 class="settings-group-title" id="settings-theme-label">Theme</h3>
    <div class="grid grid-cols-2 gap-3 sm:grid-cols-4" role="radiogroup" aria-labelledby="settings-theme-label">
      {#each themes as t (t.id)}
        {@const active = settings.theme === t.id}
        <button
          type="button"
          role="radio"
          aria-checked={active}
          class="card card-interactive flex flex-col gap-2.5 p-2 text-left {active ? 'card-selected' : ''}"
          onclick={() => onThemeChange(t.id)}
        >
          <span
            class="flex h-16 overflow-hidden rounded-md border"
            style="background: {t.surface}; border-color: {t.border};"
            aria-hidden="true"
          >
            <span class="flex w-5 shrink-0 flex-col items-center gap-1 pt-2" style="background: {t.chrome}; border-right: 1px solid {t.border};">
              <span class="size-1.5 rounded-full" style="background: {t.accent};"></span>
              <span class="size-1.5 rounded-full" style="background: {t.muted};"></span>
              <span class="size-1.5 rounded-full" style="background: {t.muted};"></span>
            </span>
            <span class="flex flex-1 flex-col gap-1.5 p-2">
              <span class="flex flex-col gap-1 rounded-xs p-1.5" style="background: {t.panel}; border: 1px solid {t.border};">
                <span class="h-1 w-3/4 rounded-full" style="background: {t.text};"></span>
                <span class="h-1 w-1/2 rounded-full" style="background: {t.muted};"></span>
              </span>
              <span class="h-2 w-6 rounded-xs" style="background: {t.accent};"></span>
            </span>
          </span>
          <span class="flex items-center justify-between px-0.5 text-sm {active ? 'text-primary' : 'text-secondary'}">
            {t.name}
            {#if active}<Check size={14} class="text-accent" />{/if}
          </span>
        </button>
      {/each}
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">Locale</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Timezone</span>
          <span class="setting-meta">Used for dates in history, logs and audit.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-timezone-filter"
            type="text"
            placeholder="Filter…"
            aria-label="Filter timezones"
            bind:value={timezoneFilter}
            class="input w-28"
          />
          <CustomSelect
            id="settings-timezone"
            class="w-56!"
            size="md"
            options={timezoneSelectOptions}
            value={settings.timezone}
            onChange={(val) => {
              settings.timezone = val;
              onSave();
            }}
          />
        </div>
      </div>
    </div>
  </section>
</div>
