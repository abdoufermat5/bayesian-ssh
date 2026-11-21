<script lang="ts">
  import { Columns2, X } from "lucide-svelte";
  import { tabDrag, type TabDragHandlers } from "$lib/actions/tabDrag";
  import { disconnectTab, getTerminalState, type TerminalTab } from "$lib/stores/terminal.svelte";
  import { tabGroupColorVar, type TabGroup } from "$lib/utils/tab-groups";
  import { statusLabel } from "$lib/utils/terminal-status";

  interface Props {
    tab: TerminalTab;
    group: TabGroup | undefined;
    active: boolean;
    inSplit: boolean;
    /** Dragged where releasing opens it in its own window. */
    poppingOut: boolean;
    drag: TabDragHandlers;
    onMenu: (e: MouseEvent | KeyboardEvent) => void;
    /** The tab was chosen (mouse or keyboard); the parent decides what to reveal. */
    onSelect: () => void;
  }

  let { tab, group, active, inSplit, poppingOut, drag, onMenu, onSelect }: Props = $props();

  const terminalState = getTerminalState();

  /** Jack lamp state: live, breathing (connecting), error, or idle (disconnected). */
  function lampClass(status: TerminalTab["status"]): string {
    if (status === "connected") return "lamp-live";
    if (status === "connecting") return "lamp-connecting";
    if (status === "error") return "lamp-error";
    return "";
  }

  const label = $derived(tab.remoteTitle?.trim() ? tab.remoteTitle : tab.name);
  const tooltip = $derived(
    tab.remoteTitle?.trim() && tab.remoteTitle.trim() !== tab.name ? `${tab.name} — ${tab.remoteTitle.trim()}` : tab.name,
  );

  /** OSC 9;4 progress: width in %, and colour by state (2 error, 4 paused). */
  const progressBar = $derived.by(() => {
    if (!tab.progress || tab.progress.state === 0 || tab.progress.state === 3) return null;
    const { state, value } = tab.progress;
    const v = Number.isFinite(value) ? value : 0;
    const width = (state === 2 || state === 4) && v <= 0 ? 100 : Math.max(0, Math.min(100, v));
    const color = state === 2 ? "bg-error" : state === 4 ? "bg-warning" : "bg-accent";
    return { width, color };
  });
</script>

<div
  data-tab-id={tab.id}
  class="group relative flex h-full max-w-48 shrink-0 cursor-pointer items-center gap-2 overflow-hidden border-r border-border-subtle pl-2.5 pr-1 text-xs transition-colors duration-fast
    {active ? 'bg-surface-terminal text-primary' : 'text-muted hover:bg-surface-hover hover:text-primary'}"
  class:tab-dragging-out={poppingOut}
  use:tabDrag={drag}
  onclick={() => {
    terminalState.activeTabId = tab.id;
    onSelect();
  }}
  onauxclick={(e) => {
    if (e.button === 1) {
      e.preventDefault();
      void disconnectTab(tab.id);
    }
  }}
  oncontextmenu={onMenu}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      terminalState.activeTabId = tab.id;
      onSelect();
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      onMenu(e);
    }
  }}
  role="tab"
  tabindex="0"
  aria-selected={active}
  title="{tooltip} · {statusLabel(tab.status)} — drag to reorder, split or open in a window; right-click for more"
>
  {#if active}
    <span class="pointer-events-none absolute inset-x-0 top-0 h-0.5 bg-accent" aria-hidden="true"></span>
  {/if}
  <span class="lamp {lampClass(tab.status)}" title={statusLabel(tab.status)} aria-hidden="true"></span>
  <span class="min-w-0 truncate font-medium">{label}</span>
  {#if inSplit}
    <Columns2 size={12} class="shrink-0 text-muted" aria-hidden="true" />
    <span class="sr-only">(split view)</span>
  {/if}
  {#if tab.bell}
    <span class="status-dot status-dot-sm status-dot-warning shrink-0" role="status" aria-label="Bell"></span>
  {/if}
  <button
    type="button"
    class="inline-flex size-6 shrink-0 items-center justify-center rounded-sm text-muted transition-colors duration-fast hover:bg-surface-hover hover:text-primary focus-visible:opacity-100
      {active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}"
    aria-label="Close {tab.name}"
    title="Close session"
    onclick={(e) => {
      e.stopPropagation();
      void disconnectTab(tab.id);
    }}
  >
    <X size={14} />
  </button>
  {#if group}
    <span
      class="tab-group-bar pointer-events-none absolute inset-x-1.5 bottom-0 h-0.5 rounded-full"
      style:--group-color={tabGroupColorVar(group.color)}
      aria-hidden="true"
    ></span>
  {/if}
  {#if tab.progress && tab.progress.state !== 0}
    <div class="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 overflow-hidden bg-border/40" aria-hidden="true">
      {#if progressBar}
        <div
          class="h-full transition-[width] duration-base motion-reduce:transition-none {progressBar.color}"
          style:width="{progressBar.width}%"
        ></div>
      {:else}
        <div class="tab-progress-indeterminate h-full w-1/3 rounded-full bg-accent motion-reduce:animate-none"></div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tab-group-bar {
    background: var(--group-color);
  }
  .tab-progress-indeterminate {
    animation: loader-slide 1.2s var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1)) infinite;
  }
  @media (prefers-reduced-motion: reduce) {
    .tab-progress-indeterminate {
      animation: none !important;
      width: 100% !important;
      opacity: 0.6;
    }
  }
</style>
