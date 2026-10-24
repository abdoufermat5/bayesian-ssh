<script lang="ts">
  import { diffStats, parseDiff, type DiffLineKind } from "$lib/utils/ai";

  interface Props {
    patch: string;
    /** What the diff changes, for the region's accessible name. */
    label: string;
  }

  let { patch, label }: Props = $props();

  const lines = $derived(parseDiff(patch));
  const stats = $derived(diffStats(lines));

  const CLASS: Record<DiffLineKind, string> = {
    add: "diff-add",
    del: "diff-del",
    hunk: "diff-hunk",
    meta: "diff-hunk",
    ctx: "",
  };
</script>

<div class="flex flex-col gap-1">
  <!-- Focusable on purpose: axe requires a scrollable region to be reachable by keyboard. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="diff" role="region" aria-label="Changes to {label}" tabindex="0">
    {#each lines as line, i (i)}
      <span class="diff-line {CLASS[line.kind]}">{line.text === "" ? " " : line.text}</span>
    {/each}
  </div>
  <span class="type-meta tabular-nums">
    {stats.added} added, {stats.removed} removed
  </span>
</div>
