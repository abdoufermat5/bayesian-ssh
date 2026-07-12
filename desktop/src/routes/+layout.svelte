<script lang="ts">
  import "@xterm/xterm/css/xterm.css";
  import { page } from "$app/state";
  import "../app.css";

  let { children } = $props();

  const isTerminalWindow = $derived(page.url.pathname.startsWith("/terminal/"));
</script>

{#if isTerminalWindow}
  {@render children?.()}
{:else}
  <div class="app-root">
    {@render children?.()}
  </div>
{/if}
