<script lang="ts">
  import { ChevronDown, ChevronUp, Minimize2, Search, TriangleAlert, X } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import { appState } from "$lib/stores/appState.svelte";
  import { HOST_SHORTCUTS, TERMINAL_SHORTCUTS } from "$lib/shortcuts";
  import {
    cancelConnect,
    closeTerminalSearch,
    disconnectTab,
    getTerminalState,
    retryConnect,
    unsplitTab,
    type TerminalTab,
  } from "$lib/stores/terminal.svelte";
  import { cssPercent, type Rect } from "$lib/utils/split-layout";
  import { focusOnMount, hostLine, statusDot } from "$lib/utils/terminal-status";

  interface Props {
    tab: TerminalTab;
    /** Place in the split, when the tab is shown beside others. */
    rect: Rect | undefined;
    active: boolean;
    /** Hosts, to find the one to edit after a failed connect. */
    connections: Connection[];
  }

  let { tab, rect, active, connections }: Props = $props();

  const terminalState = getTerminalState();

  const CANCEL_KEY = TERMINAL_SHORTCUTS.find((s) => s.id === "term-cancel")?.combos[0].join("+") ?? "Esc";
  const CONNECT_CLI = HOST_SHORTCUTS.find((s) => s.id === "host-connect")?.cli ?? "bssh connect <host>";

  let searchQuery = $state("");

  // Ticks once a second while connecting, for the elapsed time.
  let now = $state(Date.now());
  $effect(() => {
    if (tab.status !== "connecting") return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  const elapsed = $derived.by(() => {
    const seconds = Math.max(0, Math.floor((now - tab.connectStartedAt) / 1000));
    return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  });

  const target = $derived(tab.conn ? hostLine(tab.conn) : tab.name);

  function runSearch(backwards = false) {
    if (!searchQuery) return;
    if (backwards) tab.searchAddon?.findPrevious(searchQuery);
    else tab.searchAddon?.findNext(searchQuery);
  }

  function closeSearch() {
    tab.searchAddon?.clearDecorations();
    closeTerminalSearch(tab.id);
    tab.term?.focus();
  }

  function editHost() {
    const conn = connections.find((c) => c.name === tab.connectionName) ?? tab.conn;
    if (conn) appState.openEditModal(conn);
  }

  /** Clicking or focusing a split pane makes its tab the active one. */
  function activate() {
    if (rect && !active) terminalState.activeTabId = tab.id;
  }

  function focusOnShow(node: HTMLElement) {
    requestAnimationFrame(() => node.focus());
  }
</script>

<div
  class="absolute box-border overflow-hidden {rect ? 'split-pane p-1' : 'inset-0 p-2'}"
  class:split-pane-focused={!!rect && active}
  class:hidden={!rect && !active}
  style:left={rect ? cssPercent(rect.x) : undefined}
  style:top={rect ? cssPercent(rect.y) : undefined}
  style:width={rect ? cssPercent(rect.w) : undefined}
  style:height={rect ? cssPercent(rect.h) : undefined}
  role="tabpanel"
  aria-label={tab.remoteTitle ?? tab.name}
  onfocusin={activate}
  onpointerdowncapture={activate}
>
  {#if tab.showSearch}
    <div class="popover absolute right-4 top-2 z-10 flex items-center gap-0.5">
      <Search size={14} class="ml-1.5 shrink-0 text-muted" />
      <input
        type="text"
        placeholder="Find in scrollback"
        aria-label="Find in scrollback"
        class="h-7 w-52 min-w-0 bg-transparent px-1.5 text-sm text-primary outline-none placeholder:text-muted"
        bind:value={searchQuery}
        use:focusOnMount
        oninput={() => runSearch()}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            runSearch(e.shiftKey);
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            closeSearch();
          }
        }}
      />
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        aria-label="Previous match"
        title="Previous match (Shift+Enter)"
        onclick={() => runSearch(true)}
      >
        <ChevronUp size={14} />
      </button>
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        aria-label="Next match"
        title="Next match (Enter)"
        onclick={() => runSearch()}
      >
        <ChevronDown size={14} />
      </button>
      <span class="mx-0.5 h-4 w-px bg-border" aria-hidden="true"></span>
      <button type="button" class="btn-icon btn-icon-sm" aria-label="Close search" title="Close (Esc)" onclick={closeSearch}>
        <X size={14} />
      </button>
    </div>
  {/if}

  <div class="flex h-full w-full flex-col">
    {#if rect}
      <div class="flex h-6 shrink-0 items-center gap-1.5 px-1 text-xs {active ? 'text-primary' : 'text-muted'}">
        <span class="status-dot status-dot-sm {statusDot(tab.status)}" aria-hidden="true"></span>
        <span class="min-w-0 flex-1 truncate font-medium">{tab.remoteTitle?.trim() || tab.name}</span>
        <button
          type="button"
          class="btn-icon btn-icon-sm size-6"
          aria-label="Remove {tab.name} from split"
          title="Remove from split (the session keeps running)"
          onclick={() => unsplitTab(tab.id)}
        >
          <Minimize2 size={12} />
        </button>
      </div>
    {/if}
    {#if tab.status === "connecting"}
      <div
        class="mb-2 flex shrink-0 items-center gap-2 rounded-md border border-border bg-surface px-3 py-1.5 text-sm text-secondary"
        role="status"
      >
        <span class="spinner size-3" aria-hidden="true"></span>
        <span class="min-w-0 flex-1 truncate">
          Connecting to {target}{tab.conn?.bastion ? ` via ${tab.conn.bastion}` : ""}…
          <span class="tabular-nums text-muted">{elapsed}</span>
        </span>
        <button type="button" class="btn btn-ghost btn-sm" onclick={() => cancelConnect(tab.id)}>
          Cancel
          <kbd class="kbd">{CANCEL_KEY}</kbd>
        </button>
      </div>
    {/if}
    <div id="terminal-{tab.id}" class="min-h-0 w-full flex-1"></div>
  </div>

  {#if tab.status === "error" && tab.failure}
    <div class="absolute inset-0 z-10 flex items-center justify-center overflow-y-auto bg-surface-terminal p-6">
      <div class="panel flex w-full max-w-lg flex-col gap-4 p-5" role="alert">
        <div class="flex items-start gap-3">
          <TriangleAlert size={18} class="mt-0.5 shrink-0 text-error" aria-hidden="true" />
          <div class="flex min-w-0 flex-col gap-1">
            <h2 class="m-0 text-base font-semibold text-primary">
              {tab.failure.title}
            </h2>
            <p class="m-0 text-sm text-secondary">
              Couldn't connect to {target}. {tab.failure.hint}
            </p>
          </div>
        </div>
        {#if tab.failure.detail}
          <pre
            class="m-0 max-h-40 select-text overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-surface p-2.5 font-mono text-xs text-secondary">{tab.failure.detail}</pre>
        {/if}
        <div class="flex flex-wrap items-center gap-2">
          <button type="button" class="btn btn-primary" onclick={() => retryConnect(tab.id)} use:focusOnShow>
            Retry
          </button>
          <button type="button" class="btn btn-secondary" onclick={editHost} disabled={!tab.conn}>Edit host</button>
          <button type="button" class="btn btn-ghost ml-auto" onclick={() => disconnectTab(tab.id)}>
            Close tab
          </button>
        </div>
        <p class="m-0 text-xs text-muted">
          CLI: <code class="select-text">{CONNECT_CLI.replace("<host>", tab.connectionName)}</code>
        </p>
      </div>
    </div>
  {/if}
</div>

<style>
  .split-pane {
    outline: 1px solid var(--color-border);
    outline-offset: -1px;
  }
  .split-pane-focused {
    outline-color: color-mix(in srgb, var(--color-accent) 60%, transparent);
  }
</style>
