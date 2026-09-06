<script lang="ts">
  import { AppWindow, ExternalLink, Link2, Search, Server, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { DetachedSession, PopoutSession } from "$lib/stores/terminal.svelte";

  interface Props {
    detachedSessions: DetachedSession[];
    popoutSessions: PopoutSession[];
    onClose: () => void;
    onReattach: (sessionId: string) => void | Promise<void>;
    onPopOut: (sessionId: string) => void | Promise<void>;
    onDock: (sessionId: string) => void | Promise<void>;
    onFocusPopout: (sessionId: string) => void | Promise<void>;
    onTerminateDetached: (sessionId: string) => void | Promise<void>;
    onTerminatePopout: (sessionId: string) => void | Promise<void>;
    onTerminateAll: () => void | Promise<void>;
  }

  let {
    detachedSessions,
    popoutSessions,
    onClose,
    onReattach,
    onPopOut,
    onDock,
    onFocusPopout,
    onTerminateDetached,
    onTerminatePopout,
    onTerminateAll,
  }: Props = $props();

  let query = $state("");

  const totalCount = $derived(detachedSessions.length + popoutSessions.length);

  const filteredDetached = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return detachedSessions;
    return detachedSessions.filter(
      (session) =>
        session.name.toLowerCase().includes(needle) ||
        session.connectionName.toLowerCase().includes(needle),
    );
  });

  const filteredPopouts = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return popoutSessions;
    return popoutSessions.filter(
      (session) =>
        session.name.toLowerCase().includes(needle) ||
        session.connectionName.toLowerCase().includes(needle),
    );
  });
</script>

<ModalShell
  open={true}
  title="Running Sessions"
  onClose={onClose}
  width="lg"
  panelClass="max-h-[80vh]"
>
  <div class="modal-header">
    <div>
      <h3 id="session-manager-title" class="modal-title">Running Sessions</h3>
      <p class="modal-subtitle">Programs keep running when hidden. Reattach or dock to restore the terminal view.</p>
    </div>
    <button
      type="button"
      class="modal-close"
      onclick={onClose}
      aria-label="Close"
    >
      <X size={16} />
    </button>
  </div>

  <div class="flex gap-2.5 items-center px-5 py-3 border-b border-border bg-surface-input/30">
    <div class="search-box flex-1 h-8">
      <Search size={14} class="text-muted shrink-0" />
      <input
        type="text"
        placeholder="Filter sessions..."
        bind:value={query}
        class="bg-transparent border-none text-primary outline-none w-full text-xs"
      />
    </div>
    {#if totalCount > 0}
      <button
        type="button"
        class="btn btn-danger btn-sm h-8"
        onclick={onTerminateAll}
      >
        Terminate all
      </button>
    {/if}
  </div>

  <div class="modal-body flex flex-col gap-2">
      {#if filteredPopouts.length > 0}
        <div class="text-[10px] font-bold tracking-widest text-muted uppercase mt-1 mb-1 block pl-0.5">Pop-out windows</div>
        {#each filteredPopouts as session (session.id)}
          <div class="panel flex items-center justify-between gap-3 p-3 px-4">
            <div class="flex items-center gap-2.5 min-w-0 text-accent">
              <AppWindow size={14} />
              <div class="min-w-0">
                <span class="block text-xs font-semibold text-primary overflow-hidden text-ellipsis whitespace-nowrap">{session.name}</span>
                <span class="block text-[10px] text-muted">Running in separate window</span>
              </div>
            </div>
            <div class="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                title="Bring back to main tab bar"
                onclick={() => onDock(session.id)}
              >
                <Link2 size={13} />
                <span>Dock here</span>
              </button>
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                title="Focus pop-out window"
                onclick={() => onFocusPopout(session.id)}
              >
                <ExternalLink size={13} />
                <span>Focus</span>
              </button>
              <button
                type="button"
                class="btn-icon text-muted hover:text-error hover:bg-error/15"
                title="Terminate session"
                onclick={() => onTerminatePopout(session.id)}
              >
                <X size={14} />
              </button>
            </div>
          </div>
        {/each}
      {/if}

      {#if filteredDetached.length > 0}
        <div class="text-[10px] font-bold tracking-widest text-muted uppercase mt-3 mb-1 block pl-0.5">Background detached</div>
        {#each filteredDetached as session (session.id)}
          <div class="panel flex items-center justify-between gap-3 p-3 px-4">
            <div class="flex items-center gap-2.5 min-w-0 text-accent">
              <Server size={14} />
              <div class="min-w-0">
                <span class="block text-xs font-semibold text-primary overflow-hidden text-ellipsis whitespace-nowrap">{session.name}</span>
                <span class="block text-[10px] text-muted">Hidden tab — SSH process still active</span>
              </div>
            </div>
            <div class="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                title="Restore tab in main window"
                onclick={() => onReattach(session.id)}
              >
                <Link2 size={13} />
                <span>Reattach</span>
              </button>
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                title="Open in new window"
                onclick={() => onPopOut(session.id)}
              >
                <AppWindow size={13} />
                <span>Pop out</span>
              </button>
              <button
                type="button"
                class="btn-icon text-muted hover:text-error hover:bg-error/15"
                title="Terminate session"
                onclick={() => onTerminateDetached(session.id)}
              >
                <X size={14} />
              </button>
            </div>
          </div>
        {/each}
      {/if}

      {#if filteredPopouts.length === 0 && filteredDetached.length === 0}
        <div class="py-12 text-center text-muted text-xs">
          {query.trim() ? "No sessions match your filter." : "No detached or pop-out sessions."}
        </div>
      {/if}
    </div>
</ModalShell>
