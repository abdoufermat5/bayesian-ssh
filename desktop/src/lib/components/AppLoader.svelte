<script lang="ts">
  import BrandMark from "./BrandMark.svelte";
  interface Props {
    title?: string;
    subtitle?: string;
  }

  let { title = "Bayesian SSH", subtitle = "Loading hosts and settings…" }: Props = $props();
</script>

<div
  class="fixed inset-0 z-[300] flex select-none flex-col items-center justify-center gap-5 bg-chrome p-6 text-center"
  role="status"
  aria-live="polite"
>
  <BrandMark class="size-10" />

  <div class="flex flex-col gap-1">
    <h1 class="m-0 text-sm font-semibold text-primary">{title}</h1>
    <p class="m-0 text-xs text-muted">{subtitle}</p>
  </div>

  <div class="h-0.5 w-40 overflow-hidden rounded-full bg-surface-hover">
    <div class="h-full w-1/3 rounded-full bg-accent animate-[loader-slide_1.2s_var(--ease-out)_infinite]"></div>
  </div>
</div>
