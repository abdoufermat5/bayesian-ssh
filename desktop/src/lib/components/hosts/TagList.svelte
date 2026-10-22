<script lang="ts">
  import AnchoredPopover from "./AnchoredPopover.svelte";

  interface Props {
    tags: string[];
    /** Tags shown inline; the rest go behind "+N". */
    limit: number;
    /** Tab index of the "+N" button (-1 inside the host grid, which uses arrow keys). */
    tabindex?: number;
    wrap?: boolean;
  }

  let { tags, limit, tabindex = 0, wrap = false }: Props = $props();

  let open = $state(false);
  let button = $state<HTMLButtonElement>();

  const shown = $derived(tags.slice(0, limit));
  const hidden = $derived(tags.length - shown.length);
</script>

<div class="flex min-w-0 items-center gap-1 {wrap ? 'flex-wrap' : 'overflow-hidden'}">
  {#each shown as tag (tag)}
    <span class="tag min-w-0 {wrap ? '' : 'block shrink truncate leading-5'}" title={tag}>{tag}</span>
  {/each}
  {#if hidden > 0}
    <button
      bind:this={button}
      type="button"
      class="tag tag-more shrink-0 cursor-pointer"
      {tabindex}
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-label="{hidden} more {hidden === 1 ? 'tag' : 'tags'}: {tags.slice(limit).join(', ')}"
      title={tags.join(", ")}
      onclick={(e) => {
        e.stopPropagation();
        open = !open;
      }}
      ondblclick={(e) => e.stopPropagation()}
      onkeydown={(e) => {
        if (e.key === "Escape" && open) {
          e.preventDefault();
          e.stopPropagation();
          open = false;
        }
      }}
    >
      +{hidden}
    </button>
  {/if}
</div>

{#if open && button}
  <AnchoredPopover anchor={button} label="Tags" width={220} onClose={() => (open = false)}>
    <div class="section-label px-2 pb-1 pt-1.5">Tags</div>
    <ul class="m-0 flex list-none flex-wrap gap-1 p-2 pt-1">
      {#each tags as tag (tag)}
        <li class="tag">{tag}</li>
      {/each}
    </ul>
  </AnchoredPopover>
{/if}

<style>
  .tag-more {
    min-height: 24px;
    min-width: 24px;
    justify-content: center;
  }
  .tag-more:hover {
    color: var(--color-primary);
  }
</style>
