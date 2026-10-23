<script lang="ts">
  // The Bayesian SSH mark (assets/logo.svg): a prompt chevron and a block cursor on
  // a graphite tile. The cursor takes the theme accent; the tile keeps its own colours
  // so the mark reads the same on light and dark surfaces.
  let { class: className = "" }: { class?: string } = $props();
</script>

<svg viewBox="0 0 20 20" class={className} aria-hidden="true">
  <rect width="20" height="20" rx="4.5" fill="#18181b" />
  <rect x="0.5" y="0.5" width="19" height="19" rx="4" fill="none" stroke="#2c2c33" />
  <path d="M5.3 6.6 9.05 10 5.3 13.4" fill="none" stroke="#ffffff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
  <rect x="10.9" y="6.6" width="3.75" height="6.8" rx="0.6" fill="var(--color-accent)" />
</svg>
