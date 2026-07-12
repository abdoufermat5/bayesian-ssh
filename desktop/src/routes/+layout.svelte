<script lang="ts">
  import "@xterm/xterm/css/xterm.css";
  import "../app.css";

  let { children } = $props();
</script>

<div class="app-root">
  {@render children?.()}
</div>
