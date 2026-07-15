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
  <div class="flex flex-col w-full h-[100dvh] min-h-0 overflow-hidden">
    {@render children?.()}
  </div>
{/if}
