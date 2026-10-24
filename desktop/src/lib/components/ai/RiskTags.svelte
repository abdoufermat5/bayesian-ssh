<script lang="ts">
  import type { AiRiskTag } from "$lib/types";
  import { TAG_LABEL, tagBadge } from "$lib/utils/ai";

  interface Props {
    tags: AiRiskTag[];
  }

  let { tags }: Props = $props();
</script>

{#if tags.length > 0}
  <span class="inline-flex flex-wrap items-center gap-1">
    {#each tags as tag (tag)}
      <span class="badge {tagBadge(tag)}">{TAG_LABEL[tag]}</span>
    {/each}
  </span>
{/if}
