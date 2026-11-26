<script lang="ts">
  import { Check } from "lucide-svelte";
  import type { DesktopSettings } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { applyDensity, THEME_SWATCHES, type AppDensity } from "$lib/utils/theme";
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

  const FALLBACK_SWATCHES = {
    light: {
      chrome: "#f1f3f5",
      surface: "#f8f9fa",
      panel: "#ffffff",
      border: "#d8dee6",
      text: "#111418",
      muted: "#5b6470",
      accent: "#2563eb",
    },
    dark: {
      chrome: "#0b0d10",
      surface: "#111318",
      panel: "#171a21",
      border: "#272c36",
      text: "#eceff4",
      muted: "#8c96a6",
      accent: "#3b82f6",
    },
  } as const;

  const swatches = THEME_SWATCHES ?? FALLBACK_SWATCHES;

  // Swatch colors mirror the per-theme tokens in styles/tokens.css. They are a
  // preview of themes other than the active one, so they can't use utilities.
  const themes = [
    { id: "light", name: "Light", ...swatches.light },
    { id: "dark", name: "Dark", ...swatches.dark },
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

  const timezoneSelectOptions = $derived.by(() => {
    const selected = settings.timezone;
    const list =
      selected && selected !== SYSTEM_TIMEZONE && !timezoneOptions.includes(selected)
        ? [selected, ...timezoneOptions]
        : timezoneOptions;
    return [
      { value: SYSTEM_TIMEZONE, label: `System (${systemTimezone})` },
      ...list.map((tz) => ({ value: tz, label: tz })),
    ];
  });
</script>

<div class="settings-page">
  <section>
    <h2 class="settings-group-title" id="settings-theme-label">Theme</h2>
    <div
      class="grid grid-cols-1 gap-3 sm:grid-cols-3"
      role="radiogroup"
      tabindex="-1"
      aria-labelledby="settings-theme-label"
      onkeydown={(e) => {
        const idx = themes.findIndex((t) => t.id === settings.theme);
        let next = -1;
        if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (idx + 1) % themes.length;
        else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (idx - 1 + themes.length) % themes.length;
        if (next === -1) return;
        e.preventDefault();
        onThemeChange(themes[next].id);
        const buttons = (e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('[role="radio"]');
        buttons[next]?.focus();
      }}
    >
      {#each themes as t (t.id)}
        {@const active = settings.theme === t.id}
        <button
          type="button"
          role="radio"
          aria-checked={active}
          tabindex={active ? 0 : -1}
          class="card card-interactive flex flex-col gap-2.5 p-2 text-left {active ? 'card-selected' : ''}"
          onclick={() => onThemeChange(t.id)}
        >
          {#if t.id === "system"}
            <span
              class="flex h-16 overflow-hidden rounded-md border border-border"
              aria-hidden="true"
            >
              <span class="flex flex-1 overflow-hidden" style="background: {swatches.dark.surface};">
                <span class="flex w-3.5 shrink-0 flex-col items-center gap-1 pt-2" style="background: {swatches.dark.chrome}; border-right: 1px solid {swatches.dark.border};">
                  <span class="size-1 rounded-full" style="background: {swatches.dark.accent};"></span>
                  <span class="size-1 rounded-full" style="background: {swatches.dark.muted};"></span>
                </span>
                <span class="flex flex-1 flex-col gap-1 p-1.5">
                  <span class="flex flex-col gap-0.5 rounded-xs p-1" style="background: {swatches.dark.panel}; border: 1px solid {swatches.dark.border};">
                    <span class="h-1 w-3/4 rounded-full" style="background: {swatches.dark.text};"></span>
                    <span class="h-1 w-1/2 rounded-full" style="background: {swatches.dark.muted};"></span>
                  </span>
                  <span class="h-1.5 w-4 rounded-xs" style="background: {swatches.dark.accent};"></span>
                </span>
              </span>
              <span class="flex flex-1 overflow-hidden" style="background: {swatches.light.surface}; border-left: 1px solid {swatches.light.border};">
                <span class="flex flex-1 flex-col gap-1 p-1.5">
                  <span class="flex flex-col gap-0.5 rounded-xs p-1" style="background: {swatches.light.panel}; border: 1px solid {swatches.light.border};">
                    <span class="h-1 w-3/4 rounded-full" style="background: {swatches.light.text};"></span>
                    <span class="h-1 w-1/2 rounded-full" style="background: {swatches.light.muted};"></span>
                  </span>
                  <span class="h-1.5 w-4 rounded-xs" style="background: {swatches.light.accent};"></span>
                </span>
                <span class="flex w-3.5 shrink-0 flex-col items-center gap-1 pt-2" style="background: {swatches.light.chrome}; border-left: 1px solid {swatches.light.border};">
                  <span class="size-1 rounded-full" style="background: {swatches.light.accent};"></span>
                  <span class="size-1 rounded-full" style="background: {swatches.light.muted};"></span>
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
    <h2 class="settings-group-title" id="settings-density-label">Density</h2>
    <div
      class="grid grid-cols-1 gap-3 sm:grid-cols-3"
      role="radiogroup"
      tabindex="-1"
      aria-labelledby="settings-density-label"
      onkeydown={(e) => {
        const current = settings.density ?? "comfortable";
        const idx = densityOptions.findIndex((d) => d.id === current);
        let next = -1;
        if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (idx + 1) % densityOptions.length;
        else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (idx - 1 + densityOptions.length) % densityOptions.length;
        if (next === -1) return;
        e.preventDefault();
        handleDensitySelect(densityOptions[next].id);
        const buttons = (e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('[role="radio"]');
        buttons[next]?.focus();
      }}
    >
      {#each densityOptions as d (d.id)}
        {@const active = (settings.density ?? "comfortable") === d.id}
        <button
          type="button"
          role="radio"
          aria-checked={active}
          tabindex={active ? 0 : -1}
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
    <h2 class="settings-group-title">Locale</h2>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Timezone</span>
          <span class="setting-meta">Used for dates in history, logs and audit.</span>
        </div>
        <div class="setting-control">
          <CustomSelect
            id="settings-timezone"
            ariaLabel="Timezone"
            class="w-64!"
            size="md"
            searchable={true}
            searchPlaceholder="Filter timezones…"
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
