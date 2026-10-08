<script lang="ts">
  import { AppWindow, ExternalLink, Link2, Search, Unlink, X } from "lucide-svelte";
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

  function matches(session: { name: string; connectionName: string }, needle: string) {
    return (
      session.name.toLowerCase().includes(needle) ||
      session.connectionName.toLowerCase().includes(needle)
    );
  }

  const filteredDetached = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    return needle ? detachedSessions.filter((s) => matches(s, needle)) : detachedSessions;
  });

  const filteredPopouts = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    return needle ? popoutSessions.filter((s) => matches(s, needle)) : popoutSessions;
  });
</script>

<ModalShell open={true} title="Running sessions" {onClose} width="form" panelClass="max-h-[80vh]">
  <div class="modal-header">
    <div>
      <h3 id="session-manager-title" class="modal-title">Running sessions</h3>
      <p class="modal-subtitle">
        Sessions outside the tab bar keep running. Reattach or dock one to bring it back into the tab bar.
      </p>
    </div>
    <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
      <X size={16} />
    </button>
  </div>

  {#if totalCount > 3}
    <div class="shrink-0 px-5 pb-3">
      <div class="search-box">
        <Search size={14} class="shrink-0" />
        <input type="text" placeholder="Filter sessions" aria-label="Filter sessions" bind:value={query} />
      </div>
    </div>
  {/if}

  <div class="modal-body flex flex-col gap-4">
    {#if filteredPopouts.length > 0}
      <section class="flex flex-col gap-2">
        <div class="flex items-center gap-2">
          <span class="section-label">Pop-out windows</span>
          <span class="count">{filteredPopouts.length}</span>
        </div>
        <div class="panel overflow-hidden">
          {#each filteredPopouts as session, i (session.id)}
            <div class="group flex h-12 items-center gap-3 px-3 {i > 0 ? 'border-t border-border-subtle' : ''}">
              <AppWindow size={14} class="shrink-0 text-muted" />
              <div class="flex min-w-0 flex-1 flex-col">
                <span class="truncate text-sm text-primary">{session.name}</span>
                <span class="text-xs text-muted">In its own window</span>
              </div>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                title="Bring the window to the front"
                onclick={() => onFocusPopout(session.id)}
              >
                <ExternalLink size={14} />
                Focus
              </button>
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                title="Move back into the main window's tab bar"
                onclick={() => onDock(session.id)}
              >
                <Link2 size={14} />
                Dock
              </button>
              <button
                type="button"
                class="btn-icon btn-icon-danger"
                aria-label="Terminate {session.name}"
                title="Terminate session"
                onclick={() => onTerminatePopout(session.id)}
              >
                <X size={14} />
              </button>
            </div>
          {/each}
        </div>
      </section>
    {/if}

    {#if filteredDetached.length > 0}
      <section class="flex flex-col gap-2">
        <div class="flex items-center gap-2">
          <span class="section-label">Background</span>
          <span class="count">{filteredDetached.length}</span>
        </div>
        <div class="panel overflow-hidden">
          {#each filteredDetached as session, i (session.id)}
            <div class="group flex h-12 items-center gap-3 px-3 {i > 0 ? 'border-t border-border-subtle' : ''}">
              <Unlink size={14} class="shrink-0 text-muted" />
              <div class="flex min-w-0 flex-1 flex-col">
                <span class="truncate text-sm text-primary">{session.name}</span>
                <span class="text-xs text-muted">No tab open — the remote program is still running</span>
              </div>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                title="Open in a separate window"
                onclick={() => onPopOut(session.id)}
              >
                <AppWindow size={14} />
                Pop out
              </button>
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                title="Restore as a tab in the main window"
                onclick={() => onReattach(session.id)}
              >
                <Link2 size={14} />
                Reattach
              </button>
              <button
                type="button"
                class="btn-icon btn-icon-danger"
                aria-label="Terminate {session.name}"
                title="Terminate session"
                onclick={() => onTerminateDetached(session.id)}
              >
                <X size={14} />
              </button>
            </div>
          {/each}
        </div>
      </section>
    {/if}

    {#if filteredPopouts.length === 0 && filteredDetached.length === 0}
      <div class="empty-state py-10">
        <p class="empty-state-title">{query.trim() ? "No matching sessions" : "Nothing running elsewhere"}</p>
        <p class="empty-state-desc">
          {query.trim()
            ? "Try a different filter."
            : "Pop out or detach a terminal tab and it will be listed here."}
        </p>
      </div>
    {/if}
  </div>

  <div class="modal-footer">
    {#if totalCount > 0}
      <button type="button" class="btn btn-danger-ghost mr-auto" onclick={onTerminateAll}>
        Terminate all
      </button>
    {/if}
    <button type="button" class="btn btn-secondary" onclick={onClose}>Done</button>
  </div>
</ModalShell>
