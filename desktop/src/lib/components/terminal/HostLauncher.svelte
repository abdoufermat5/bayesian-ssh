<script lang="ts">
  import { Search, X } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { getTerminalState, ungroupTabGroup } from "$lib/stores/terminal.svelte";
  import type { SplitSide } from "$lib/utils/split-layout";
  import { tabGroupColorVar, tabGroupLabel } from "$lib/utils/tab-groups";
  import { focusOnMount, hostLine } from "$lib/utils/terminal-status";

  interface Props {
    connections: Connection[];
    /** Side of the active tab the session opens on; null for a plain tab. */
    split: SplitSide | null;
    /** Triggering button to position the popover against. */
    anchor?: HTMLElement | null;
    onConnect: (conn: Connection) => void;
    /** Connect to every remembered host of a saved group. */
    onOpenGroup: (groupId: string) => void;
  }

  let { connections, split, anchor = null, onConnect, onOpenGroup }: Props = $props();

  const terminalState = getTerminalState();

  const POPOVER_W = 288;
  const PAD = 8;
  const GAP = 4;

  let popoverEl = $state<HTMLDivElement | null>(null);
  let posStyle = $state("");
  let query = $state("");
  let index = $state(0);

  function computePosition(): string {
    const w = popoverEl?.offsetWidth || POPOVER_W;
    const h = popoverEl?.offsetHeight || 320;
    if (!anchor) {
      return `top: 40px; left: ${PAD}px;`;
    }
    const r = anchor.getBoundingClientRect();
    const prefersRightAlign = r.left > window.innerWidth / 2 || r.left + w > window.innerWidth - PAD;
    const rawLeft = prefersRightAlign ? r.right - w : r.left;
    const left = Math.max(PAD, Math.min(rawLeft, window.innerWidth - w - PAD));
    const belowFits = r.bottom + GAP + h <= window.innerHeight - PAD;
    const rawTop = belowFits ? r.bottom + GAP : r.top - GAP - h;
    const top = Math.max(PAD, Math.min(rawTop, window.innerHeight - h - PAD));
    return `top: ${Math.round(top)}px; left: ${Math.round(left)}px;`;
  }

  function updatePosition() {
    posStyle = computePosition();
  }

  $effect(() => {
    void anchor;
    void split;
    void results.length;
    updatePosition();
  });

  const initialStyle = $derived(posStyle || computePosition());

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return connections.slice(0, 8);
    return connections
      .filter(
        (c) => c.name.toLowerCase().includes(q) || c.host.toLowerCase().includes(q) || c.user.toLowerCase().includes(q),
      )
      .slice(0, 30);
  });

  /** Remembered groups with no open session. */
  const closedGroups = $derived(
    terminalState.groups.filter((g) => g.hosts.length > 0 && !terminalState.tabs.some((t) => t.groupId === g.id)),
  );

  const SIDE_TEXT: Record<SplitSide, string> = { right: "right of", left: "left of", top: "above", bottom: "below" };

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      index = Math.min(index + 1, results.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      index = Math.max(index - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const conn = results[index];
      if (conn) onConnect(conn);
    }
  }
</script>

<svelte:window onresize={updatePosition} />

<div
  bind:this={popoverEl}
  class="popover fixed z-50 flex w-72 flex-col gap-1"
  style={initialStyle}
>
  {#if split && terminalState.activeTab}
    <p class="m-0 truncate px-1.5 pt-0.5 text-xs text-muted">
      Opens {SIDE_TEXT[split]}
      {terminalState.activeTab.name}
    </p>
  {/if}
  <div class="search-box h-8">
    <Search size={14} class="shrink-0" aria-hidden="true" />
    <input
      type="text"
      role="combobox"
      aria-expanded="true"
      aria-controls="host-launcher-listbox"
      aria-autocomplete="list"
      aria-activedescendant={results[index] ? `host-launcher-opt-${results[index].id}` : undefined}
      placeholder="Connect to host…"
      aria-label="Filter hosts"
      bind:value={query}
      oninput={() => (index = 0)}
      onkeydown={onKeydown}
      use:focusOnMount
    />
  </div>
  <div id="host-launcher-listbox" class="flex max-h-72 flex-col overflow-y-auto" role="listbox" aria-label="Hosts">
    {#each results as conn, i (conn.id)}
      <button
        type="button"
        id="host-launcher-opt-{conn.id}"
        tabindex="-1"
        class="menu-item h-auto flex-col items-start gap-0 py-1.5 {i === index ? 'menu-item-active' : ''}"
        role="option"
        aria-selected={i === index}
        onmouseenter={() => (index = i)}
        onclick={() => onConnect(conn)}
      >
        <span class="w-full truncate text-sm text-primary">{conn.name}</span>
        <span class="mono w-full truncate text-muted">{hostLine(conn)}</span>
      </button>
    {:else}
      <p class="px-2 py-3 text-center text-xs text-muted">No matching hosts</p>
    {/each}
  </div>
  {#if !split && closedGroups.length > 0 && !query.trim()}
    <div class="menu-separator"></div>
    <div class="section-label px-2 pt-1">Saved groups</div>
    {#each closedGroups as group (group.id)}
      {@const label = tabGroupLabel(group)}
      <div class="flex items-center gap-0.5">
        <button
          type="button"
          class="menu-item min-w-0 flex-1"
          title="Connect to every host of this group"
          onclick={() => onOpenGroup(group.id)}
        >
          <span class="tab-group-dot" style:--group-color={tabGroupColorVar(group.color)} aria-hidden="true"></span>
          <span class="min-w-0 flex-1 truncate">Open {label}</span>
          <span class="shrink-0 text-xs text-muted">
            {group.hosts.length} {group.hosts.length === 1 ? "host" : "hosts"}
          </span>
        </button>
        <button
          type="button"
          class="btn-icon btn-icon-sm shrink-0"
          aria-label="Forget group {label}"
          title="Forget group"
          onclick={() => ungroupTabGroup(group.id)}
        >
          <X size={14} />
        </button>
      </div>
    {/each}
  {/if}
</div>
