<script lang="ts">
  import { onMount } from "svelte";
  import "../app.css";

  let { children } = $props();

  onMount(() => {
    document.documentElement.classList.add("theme-zinc");
  });
</script>

<div class="app-root">
  {@render children?.()}
</div>
