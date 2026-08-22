<script lang="ts">
  import { ShieldCheck } from "lucide-svelte";

  interface Props {
    title?: string;
    subtitle?: string;
  }

  let {
    title = "Initializing Bayesian SSH",
    subtitle = "Loading environment, security policies, and connection registry...",
  }: Props = $props();
</script>

<div
  class="fixed inset-0 z-[300] flex flex-col items-center justify-center gap-6 bg-surface p-6 text-center select-none overflow-hidden"
>
  <!-- Logo mark -->
  <div class="relative flex items-center justify-center">
    <div
      class="flex h-16 w-16 items-center justify-center rounded-2xl border border-border bg-panel shadow-md"
    >
      <ShieldCheck size={28} class="text-accent" />
    </div>
    <div class="pointer-events-none absolute -inset-1.5 rounded-3xl border border-border/60"></div>
  </div>

  <!-- Title & subtitle -->
  <div class="flex flex-col gap-1">
    <h2 class="m-0 text-base font-semibold tracking-tight text-primary">{title}</h2>
    <p class="m-0 text-xs text-muted">{subtitle}</p>
  </div>

  <!-- Indeterminate progress: subtle, one moving element -->
  <div class="h-1 w-44 overflow-hidden rounded-full bg-surface-input">
    <div
      class="h-full w-1/3 rounded-full bg-accent/70 animate-[loader-slide_1.2s_var(--ease-out)_infinite]"
    ></div>
  </div>
</div>
