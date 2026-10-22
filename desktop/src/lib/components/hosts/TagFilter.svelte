<script lang="ts">
  import AnchoredPopover from "./AnchoredPopover.svelte";

  interface Props {
    tags: string[];
    selected: string | null;
    onSelect: (tag: string | null) => void;
  }

  let { tags, selected, onSelect }: Props = $props();

  /** Room kept at the end of the row for the "+N" button. */
  const MORE_BUTTON_PX = 52;
  const GAP_PX = 6;

  let row = $state<HTMLDivElement>();
  let measure = $state<HTMLDivElement>();
  let visibleCount = $state(Number.POSITIVE_INFINITY);
  let open = $state(false);
  let moreButton = $state<HTMLButtonElement>();

  // Chips are measured in an invisible copy of the row, so the visible row can
  // show exactly the ones that fit and put the rest behind "+N".
  function fit() {
    if (!row || !measure) return;
    const available = row.clientWidth;
    const widths = Array.from(measure.children).map((c) => (c as HTMLElement).offsetWidth);
    const total = widths.reduce((sum, w) => sum + w + GAP_PX, 0);
    if (total <= available) {
      visibleCount = Number.POSITIVE_INFINITY;
      return;
    }
    // The first chip is "All"; count how many tags fit after it.
    let used = widths[0] + GAP_PX + MORE_BUTTON_PX;
    let count = 0;
    for (const w of widths.slice(1)) {
      if (used + w + GAP_PX > available) break;
      used += w + GAP_PX;
      count += 1;
    }
    visibleCount = count;
  }

  $effect(() => {
    if (!row) return;
    void tags.length;
    void selected;
    const observer = new ResizeObserver(() => fit());
    observer.observe(row);
    fit();
    return () => observer.disconnect();
  });

  const shown = $derived(tags.slice(0, visibleCount));
  const hidden = $derived(tags.slice(shown.length));
  const selectedHidden = $derived(selected !== null && hidden.includes(selected));

  function pick(tag: string | null) {
    onSelect(tag);
    open = false;
  }
</script>

<div bind:this={row} class="relative flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden" role="group" aria-label="Filter by tag">
  <div bind:this={measure} class="pointer-events-none invisible absolute left-0 top-0 flex gap-1.5" aria-hidden="true">
    <span class="chip">All</span>
    {#each tags as tag (tag)}
      <span class="chip {selected === tag ? 'chip-active' : ''}">{tag}</span>
    {/each}
  </div>

  <button
    type="button"
    class="chip shrink-0 {selected === null ? 'chip-active' : ''}"
    onclick={() => pick(null)}
    aria-pressed={selected === null}
  >
    All
  </button>
  {#each shown as tag (tag)}
    <button
      type="button"
      class="chip shrink-0 {selected === tag ? 'chip-active' : ''}"
      onclick={() => pick(selected === tag ? null : tag)}
      aria-pressed={selected === tag}
    >
      {tag}
    </button>
  {/each}
  {#if hidden.length > 0}
    <button
      bind:this={moreButton}
      type="button"
      class="chip shrink-0 {selectedHidden ? 'chip-active' : ''}"
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-label={selectedHidden
        ? `${hidden.length} more tags, ${selected} selected`
        : `${hidden.length} more ${hidden.length === 1 ? "tag" : "tags"}`}
      onclick={() => (open = !open)}
    >
      {selectedHidden ? selected : `+${hidden.length}`}
    </button>
  {/if}
</div>

{#if open && moreButton}
  <AnchoredPopover anchor={moreButton} label="More tags" width={240} onClose={() => (open = false)}>
    <div class="section-label px-2 pb-1 pt-1.5">More tags</div>
    <div class="flex flex-wrap gap-1.5 p-2 pt-1">
      {#each hidden as tag (tag)}
        <button
          type="button"
          class="chip {selected === tag ? 'chip-active' : ''}"
          onclick={() => pick(selected === tag ? null : tag)}
          aria-pressed={selected === tag}
        >
          {tag}
        </button>
      {/each}
    </div>
  </AnchoredPopover>
{/if}
