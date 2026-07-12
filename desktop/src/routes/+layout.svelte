<script lang="ts">
  import "../app.css";

  let { children } = $props();
</script>

<div class="app-root">
  {@render children?.()}
</div>
