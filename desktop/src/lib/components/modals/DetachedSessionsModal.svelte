<script lang="ts">
  import { AppWindow, ExternalLink, Link2, Search, Server, X } from "lucide-svelte";
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

<div class="fixed inset-0 flex items-center justify-center z-[100]">
  <button
    type="button"
    class="absolute inset-0 bg-black/75 backdrop-blur-sm border-none p-0 cursor-default"
    onclick={onClose}
    aria-label="Close dialog"
  ></button>
  <div
    class="relative bg-surface border border-border rounded-2xl w-[720px] max-h-[80vh] shadow-xl flex flex-col animate-[modal-enter_0.25s_cubic-bezier(0.16,1,0.3,1)_forwards]"
    role="dialog"
    aria-modal="true"
    aria-labelledby="session-manager-title"
    tabindex="-1"
  >
    <div class="flex justify-between items-center px-6 py-5 border-b border-border">
      <div>
        <h3 id="session-manager-title" class="text-base font-semibold tracking-tight m-0 text-primary">Running Sessions</h3>
        <p class="text-xs text-muted mt-0.5">Programs keep running when hidden. Reattach or dock to restore the terminal view.</p>
      </div>
      <button
        type="button"
        class="bg-transparent border-none text-muted cursor-pointer flex p-1 rounded-md transition-all duration-100 hover:text-primary hover:bg-white/5"
        onclick={onClose}
        aria-label="Close"
      >
        <X size={16} />
      </button>
    </div>

    <div class="flex gap-2.5 items-center px-6 py-3 border-b border-border">
      <div class="flex-1 flex items-center bg-surface-input border border-border rounded-lg px-3 py-1.5 focus-within:border-border-focus focus-within:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]">
        <Search size={14} class="text-muted mr-2 shrink-0" />
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
          class="py-1.5 px-3 rounded-lg border border-danger/35 bg-danger/8 text-red-200 text-xs font-semibold cursor-pointer transition-colors duration-100 hover:bg-danger/15 hover:border-danger/50"
          onclick={onTerminateAll}
        >
          Terminate all
        </button>
      {/if}
    </div>

    <div class="overflow-y-auto px-6 py-5 flex flex-col gap-2">
      {#if filteredPopouts.length > 0}
        <div class="text-[10px] font-bold tracking-widest text-muted uppercase mt-1 mb-1 block pl-0.5">Pop-out windows</div>
        {#each filteredPopouts as session (session.id)}
          <div class="flex items-center justify-between gap-3 p-3 px-4 border border-border rounded-xl bg-white/[0.02]">
            <div class="flex items-center gap-2.5 min-w-0 text-accent">
              <AppWindow size={14} />
              <div class="min-w-0">
                <span class="block text-xs font-semibold text-primary overflow-hidden text-ellipsis whitespace-nowrap">{session.name}</span>
                <span class="block text-[10px] text-muted">Running in separate window</span>
              </div>
            </div>
            <div class="flex gap-1.5 shrink-0">
              <button
                type="button"
                class="inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-lg border border-border bg-transparent text-secondary text-[11px] cursor-pointer transition-colors duration-100 hover:text-primary hover:bg-white/5"
                title="Bring back to main tab bar"
                onclick={() => onDock(session.id)}
              >
                <Link2 size={14} />
                <span>Dock here</span>
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-lg border border-border bg-transparent text-secondary text-[11px] cursor-pointer transition-colors duration-100 hover:text-primary hover:bg-white/5"
                title="Focus pop-out window"
                onclick={() => onFocusPopout(session.id)}
              >
                <ExternalLink size={14} />
                <span>Focus</span>
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-lg border border-border bg-transparent text-secondary text-[11px] cursor-pointer transition-colors duration-100 hover:border-danger/35 hover:bg-danger/8 hover:text-red-300"
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
          <div class="flex items-center justify-between gap-3 p-3 px-4 border border-border rounded-xl bg-white/[0.02]">
            <div class="flex items-center gap-2.5 min-w-0 text-accent">
              <Server size={14} />
              <div class="min-w-0">
                <span class="block text-xs font-semibold text-primary overflow-hidden text-ellipsis whitespace-nowrap">{session.name}</span>
                <span class="block text-[10px] text-muted">Hidden tab — SSH process still active</span>
              </div>
            </div>
            <div class="flex gap-1.5 shrink-0">
              <button
                type="button"
                class="inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-lg border border-border bg-transparent text-secondary text-[11px] cursor-pointer transition-colors duration-100 hover:text-primary hover:bg-white/5"
                title="Restore tab in main window"
                onclick={() => onReattach(session.id)}
              >
                <Link2 size={14} />
                <span>Reattach</span>
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-lg border border-border bg-transparent text-secondary text-[11px] cursor-pointer transition-colors duration-100 hover:text-primary hover:bg-white/5"
                title="Open in new window"
                onclick={() => onPopOut(session.id)}
              >
                <AppWindow size={14} />
                <span>Pop out</span>
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 py-1.5 px-2.5 rounded-lg border border-border bg-transparent text-secondary text-[11px] cursor-pointer transition-colors duration-100 hover:border-danger/35 hover:bg-danger/8 hover:text-red-300"
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
  </div>
</div>
