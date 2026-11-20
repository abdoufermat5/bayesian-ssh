<script lang="ts">
  import { resizeSplit } from "$lib/stores/terminal.svelte";
  import { clampRatio, cssPercent, type DividerBox } from "$lib/utils/split-layout";

  interface Props {
    dividers: DividerBox[];
    /** Terminal area the layout fractions are relative to. */
    area: HTMLElement | null;
  }

  let { dividers, area }: Props = $props();

  /** Narrowest a split pane may be dragged, in pixels. */
  const MIN_PANE_PX = 160;

  function startResize(e: PointerEvent, divider: DividerBox) {
    if (e.button !== 0 || !area) return;
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const box = area.getBoundingClientRect();
    const row = divider.direction === "row";
    const start = row ? box.left + divider.rect.x * box.width : box.top + divider.rect.y * box.height;
    const size = row ? divider.rect.w * box.width : divider.rect.h * box.height;
    const minRatio = Math.min(0.5, MIN_PANE_PX / size);
    const onMove = (ev: PointerEvent) => {
      const ratio = ((row ? ev.clientX : ev.clientY) - start) / size;
      resizeSplit(divider.splitId, Math.min(1 - minRatio, Math.max(minRatio, ratio)));
    };
    const onEnd = () => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onEnd);
      handle.removeEventListener("pointercancel", onEnd);
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onEnd);
    handle.addEventListener("pointercancel", onEnd);
  }

  const KEY_STEPS: Record<string, number> = { ArrowLeft: -0.05, ArrowUp: -0.05, ArrowRight: 0.05, ArrowDown: 0.05 };

  function keyResize(e: KeyboardEvent, divider: DividerBox) {
    let ratio: number | null = null;
    if (e.key in KEY_STEPS) ratio = divider.ratio + KEY_STEPS[e.key];
    else if (e.key === "Home") ratio = 0;
    else if (e.key === "End") ratio = 1;
    if (ratio === null) return;
    e.preventDefault();
    resizeSplit(divider.splitId, clampRatio(ratio));
  }
</script>

{#each dividers as divider (divider.splitId)}
  {@const row = divider.direction === "row"}
  {@const at = row ? divider.rect.x + divider.rect.w * divider.ratio : divider.rect.y + divider.rect.h * divider.ratio}
  <!-- A focusable separator is an ARIA widget (window splitter): arrows resize it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="split-divider absolute z-20 {row ? 'cursor-col-resize' : 'cursor-row-resize'}"
    style:left={row ? `calc(${cssPercent(at)} - 3px)` : cssPercent(divider.rect.x)}
    style:top={row ? cssPercent(divider.rect.y) : `calc(${cssPercent(at)} - 3px)`}
    style:width={row ? "6px" : cssPercent(divider.rect.w)}
    style:height={row ? cssPercent(divider.rect.h) : "6px"}
    role="separator"
    tabindex="0"
    aria-orientation={row ? "vertical" : "horizontal"}
    aria-label="Resize split"
    aria-valuemin={10}
    aria-valuemax={90}
    aria-valuenow={Math.round(divider.ratio * 100)}
    title="Drag to resize, double-click to even out"
    onpointerdown={(e) => startResize(e, divider)}
    onkeydown={(e) => keyResize(e, divider)}
    ondblclick={() => resizeSplit(divider.splitId, 0.5)}
  ></div>
{/each}

<style>
  .split-divider {
    touch-action: none;
  }
  .split-divider::after {
    content: "";
    position: absolute;
    inset: 2px;
    border-radius: 2px;
    transition: background-color var(--transition-duration-fast);
  }
  .split-divider:hover::after,
  .split-divider:focus-visible::after,
  .split-divider:active::after {
    background: var(--color-accent);
  }
</style>
