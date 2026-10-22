<script lang="ts">
  import { Check } from "lucide-svelte";
  import type { DesktopSettings } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { applyDensity, type AppDensity } from "$lib/utils/theme";
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
    { id: "light", name: "Light", chrome: "#e9ebef", surface: "#f1f2f4", panel: "#ffffff", border: "#dde0e5", text: "#111827", muted: "#565d6b", accent: "#2563eb" },
    { id: "dark", name: "Dark", chrome: "#0c0c0e", surface: "#111113", panel: "#161618", border: "#242427", text: "#ececef", muted: "#96969e", accent: "#5b8def" },
    { id: "system", name: "Match system", chrome: "", surface: "", panel: "", border: "", text: "", muted: "", accent: "" },
  ];

  const densityOptions: { id: AppDensity; label: string; desc: string; height: string }[] = [
    { id: "compact", label: "Compact", desc: "Dense rows for data-heavy views", height: "32 px" },
    { id: "comfortable", label: "Comfortable", desc: "Balanced spacing for everyday use", height: "40 px" },
    { id: "spacious", label: "Spacious", desc: "Relaxed spacing, touch-friendly", height: "48 px" },
  ];

  function handleDensitySelect(id: AppDensity) {
    settings.density = id;
    applyDensity(id);
    onSave();
  }

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
    <p class="settings-desc">Color theme, row density and how dates are displayed.</p>
  </div>

  <section>
    <h3 class="settings-group-title" id="settings-theme-label">Theme</h3>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3" role="radiogroup" aria-labelledby="settings-theme-label">
      {#each themes as t (t.id)}
        {@const active = settings.theme === t.id}
        <button
          type="button"
          role="radio"
          aria-checked={active}
          class="card card-interactive flex flex-col gap-2.5 p-2 text-left {active ? 'card-selected' : ''}"
          onclick={() => onThemeChange(t.id)}
        >
          {#if t.id === "system"}
            <span
              class="flex h-16 overflow-hidden rounded-md border border-border"
              aria-hidden="true"
            >
              <span class="flex flex-1 overflow-hidden" style="background: #111113;">
                <span class="flex w-3.5 shrink-0 flex-col items-center gap-1 pt-2" style="background: #0c0c0e; border-right: 1px solid #242427;">
                  <span class="size-1 rounded-full" style="background: #5b8def;"></span>
                  <span class="size-1 rounded-full" style="background: #96969e;"></span>
                </span>
                <span class="flex flex-1 flex-col gap-1 p-1.5">
                  <span class="flex flex-col gap-0.5 rounded-xs p-1" style="background: #161618; border: 1px solid #242427;">
                    <span class="h-1 w-3/4 rounded-full" style="background: #ececef;"></span>
                    <span class="h-1 w-1/2 rounded-full" style="background: #96969e;"></span>
                  </span>
                  <span class="h-1.5 w-4 rounded-xs" style="background: #5b8def;"></span>
                </span>
              </span>
              <span class="flex flex-1 overflow-hidden" style="background: #f1f2f4; border-left: 1px solid #dde0e5;">
                <span class="flex flex-1 flex-col gap-1 p-1.5">
                  <span class="flex flex-col gap-0.5 rounded-xs p-1" style="background: #ffffff; border: 1px solid #dde0e5;">
                    <span class="h-1 w-3/4 rounded-full" style="background: #111827;"></span>
                    <span class="h-1 w-1/2 rounded-full" style="background: #565d6b;"></span>
                  </span>
                  <span class="h-1.5 w-4 rounded-xs" style="background: #2563eb;"></span>
                </span>
                <span class="flex w-3.5 shrink-0 flex-col items-center gap-1 pt-2" style="background: #e9ebef; border-left: 1px solid #dde0e5;">
                  <span class="size-1 rounded-full" style="background: #2563eb;"></span>
                  <span class="size-1 rounded-full" style="background: #565d6b;"></span>
                </span>
              </span>
            </span>
          {:else}
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
          {/if}
          <span class="flex items-center justify-between px-0.5 text-sm {active ? 'text-primary' : 'text-secondary'}">
            {t.name}
            {#if active}<Check size={14} class="text-accent" />{/if}
          </span>
        </button>
      {/each}
    </div>
  </section>

  <section>
    <h3 class="settings-group-title" id="settings-density-label">Density</h3>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3" role="radiogroup" aria-labelledby="settings-density-label">
      {#each densityOptions as d (d.id)}
        {@const active = (settings.density ?? "comfortable") === d.id}
        <button
          type="button"
          role="radio"
          aria-checked={active}
          class="card card-interactive flex flex-col gap-2 p-3 text-left {active ? 'card-selected' : ''}"
          onclick={() => handleDensitySelect(d.id)}
        >
          <div class="flex items-center justify-between">
            <span class="text-sm font-medium {active ? 'text-primary' : 'text-secondary'}">
              {d.label}
            </span>
            {#if active}<Check size={14} class="text-accent" />{/if}
          </div>
          <div class="flex items-center gap-2">
            <span class="badge badge-neutral text-2xs">{d.height}</span>
            <span class="text-xs text-muted">{d.desc}</span>
          </div>
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
