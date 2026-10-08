<script lang="ts">
  import { Check, Copy, Play, Plus, Search, Trash2, X } from "lucide-svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  export interface SnippetItem {
    id: string;
    title: string;
    command: string;
    category: string;
    description?: string;
  }

  interface Props {
    show: boolean;
    onClose: () => void;
    onRunSnippet: (command: string) => void;
  }

  let { show, onClose, onRunSnippet }: Props = $props();

  const STORAGE_KEY = "bayesian-ssh-snippets";

  const DEFAULT_SNIPPETS: SnippetItem[] = [
    {
      id: "1",
      title: "Tail system error logs",
      command: "tail -n 100 -f /var/log/syslog | grep -i error",
      category: "Logs",
      description: "Follow syslog, filtered to error lines.",
    },
    {
      id: "2",
      title: "Disk usage and largest directories",
      command: "df -h && echo '--- Top Directories ---' && du -sh * 2>/dev/null | sort -rh | head -n 10",
      category: "System",
      description: "Mounted filesystems plus the 10 largest folders in the current path.",
    },
    {
      id: "3",
      title: "Follow Docker container logs",
      command: "docker logs --tail 100 -f {{container_name}}",
      category: "Docker",
      description: "Stream the last 100 lines and new output from a container.",
    },
    {
      id: "4",
      title: "Nginx status and errors",
      command: "systemctl status nginx --no-pager && tail -n 20 /var/log/nginx/error.log",
      category: "Services",
      description: "Service status and the last 20 error log entries.",
    },
    {
      id: "5",
      title: "Listening ports",
      command: "ss -tulpn | grep LISTEN",
      category: "Network",
      description: "Listening TCP/UDP sockets with owning process.",
    },
    {
      id: "6",
      title: "Top processes by CPU",
      command: "ps aux --sort=-%cpu | head -n 10",
      category: "System",
      description: "The 10 processes using the most CPU.",
    },
  ];

  function loadSnippets(): SnippetItem[] {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) return JSON.parse(raw) as SnippetItem[];
    } catch {
      // fall back to defaults
    }
    return structuredClone(DEFAULT_SNIPPETS);
  }

  let snippets = $state<SnippetItem[]>(loadSnippets());
  let searchQuery = $state("");
  let selectedCategory = $state("All");
  let selectedId = $state<string | null>(snippets[0]?.id ?? null);
  let copiedId = $state<string | null>(null);

  function persist() {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(snippets));
    } catch {
      // ignore storage quota
    }
  }

  const categories = $derived.by(() => {
    const set = new Set<string>(["All"]);
    for (const s of snippets) if (s.category.trim()) set.add(s.category.trim());
    return Array.from(set);
  });

  const filteredSnippets = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    return snippets.filter(
      (s) =>
        (selectedCategory === "All" || s.category.trim() === selectedCategory) &&
        (!q ||
          s.title.toLowerCase().includes(q) ||
          s.command.toLowerCase().includes(q) ||
          (s.description?.toLowerCase().includes(q) ?? false)),
    );
  });

  const selected = $derived(snippets.find((s) => s.id === selectedId) ?? null);

  function addSnippet() {
    const item: SnippetItem = {
      id: crypto.randomUUID(),
      title: "Untitled snippet",
      command: "",
      category: selectedCategory === "All" ? "General" : selectedCategory,
      description: "",
    };
    snippets.unshift(item);
    searchQuery = "";
    selectedId = item.id;
    persist();
    requestAnimationFrame(() => document.getElementById("snippet-title")?.focus());
  }

  function deleteSnippet(id: string) {
    const index = snippets.findIndex((s) => s.id === id);
    if (index === -1) return;
    snippets.splice(index, 1);
    selectedId = snippets[Math.min(index, snippets.length - 1)]?.id ?? null;
    if (!categories.includes(selectedCategory)) selectedCategory = "All";
    persist();
    notify("Snippet deleted", "info");
  }

  function copySnippet(s: SnippetItem) {
    copyTextWithFallback(s.command);
    copiedId = s.id;
    setTimeout(() => (copiedId = null), 2000);
    notify("Command copied to clipboard", "info");
  }

  function handleRun(s: SnippetItem) {
    if (!s.command.trim()) return;
    onRunSnippet(s.command);
    onClose();
  }
</script>

{#if show}
  <ModalShell
    open={show}
    title="Snippets"
    {onClose}
    closeOnBackdrop={false}
    width="lg"
    panelStyle="max-width: 920px; height: min(620px, calc(100dvh - 48px));"
  >
    <header class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Snippets</h2>
        <p class="modal-subtitle">Saved commands you can copy or send to the active terminal.</p>
      </div>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </header>

    <div class="flex min-h-0 flex-1 border-t border-border">
      <!-- List pane -->
      <aside class="flex w-72 shrink-0 flex-col border-r border-border">
        <div class="flex flex-col gap-2 p-3">
          <div class="flex items-center gap-2">
            <label class="search-box flex-1">
              <Search size={14} class="shrink-0" />
              <input type="text" placeholder="Search snippets" aria-label="Search snippets" bind:value={searchQuery} />
            </label>
            <button type="button" class="btn-icon" aria-label="New snippet" title="New snippet" onclick={addSnippet}>
              <Plus size={14} />
            </button>
          </div>
          <div class="flex flex-wrap gap-1">
            {#each categories as cat (cat)}
              <button
                type="button"
                class="chip h-6 px-2 {selectedCategory === cat ? 'chip-active' : ''}"
                aria-pressed={selectedCategory === cat}
                onclick={() => (selectedCategory = cat)}
              >
                {cat}
              </button>
            {/each}
          </div>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-2" role="listbox" aria-label="Snippets">
          {#each filteredSnippets as s (s.id)}
            <button
              type="button"
              role="option"
              aria-selected={s.id === selectedId}
              class="flex w-full cursor-pointer flex-col gap-0.5 rounded-md px-2.5 py-2 text-left transition-colors duration-fast {s.id === selectedId
                ? 'bg-surface-active'
                : 'hover:bg-surface-hover'}"
              onclick={() => (selectedId = s.id)}
              ondblclick={() => handleRun(s)}
            >
              <span class="truncate text-sm {s.id === selectedId ? 'text-primary' : 'text-secondary'}">
                {s.title || "Untitled snippet"}
              </span>
              <span class="truncate font-mono text-xs text-muted">{s.command || "No command"}</span>
            </button>
          {:else}
            <p class="m-0 px-2.5 py-6 text-center text-xs text-muted">No snippets match.</p>
          {/each}
        </div>
      </aside>

      <!-- Editor pane -->
      <section class="flex min-w-0 flex-1 flex-col">
        {#if selected}
          <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-5">
            <div class="grid grid-cols-[1fr_180px] gap-3">
              <div class="field">
                <label class="field-label" for="snippet-title">Title</label>
                <input id="snippet-title" class="input" bind:value={selected.title} oninput={persist} />
              </div>
              <div class="field">
                <label class="field-label" for="snippet-category">Category</label>
                <input id="snippet-category" class="input" bind:value={selected.category} oninput={persist} />
              </div>
            </div>
            <div class="field">
              <label class="field-label" for="snippet-description">Description</label>
              <input
                id="snippet-description"
                class="input"
                placeholder="Optional"
                bind:value={selected.description}
                oninput={persist}
              />
            </div>
            <div class="field min-h-0 flex-1">
              <label class="field-label" for="snippet-command">Command</label>
              <textarea
                id="snippet-command"
                class="input input-mono min-h-32 flex-1 resize-none"
                spellcheck="false"
                placeholder="e.g. journalctl -u nginx -n 50"
                bind:value={selected.command}
                oninput={persist}
              ></textarea>
              <span class="field-hint">Placeholders such as <span class="mono">{"{{container_name}}"}</span> are sent as typed.</span>
            </div>
          </div>

          <footer class="modal-footer justify-between">
            <button type="button" class="btn btn-danger-ghost btn-sm" onclick={() => deleteSnippet(selected.id)}>
              <Trash2 size={14} />
              Delete
            </button>
            <div class="flex items-center gap-2">
              <button
                type="button"
                class="btn btn-secondary"
                disabled={!selected.command.trim()}
                onclick={() => copySnippet(selected)}
              >
                {#if copiedId === selected.id}
                  <Check size={14} class="text-success" />
                  Copied
                {:else}
                  <Copy size={14} />
                  Copy
                {/if}
              </button>
              <button
                type="button"
                class="btn btn-primary"
                disabled={!selected.command.trim()}
                onclick={() => handleRun(selected)}
              >
                <Play size={14} />
                Run in terminal
              </button>
            </div>
          </footer>
        {:else}
          <div class="empty-state flex-1">
            <div class="empty-state-title">No snippet selected</div>
            <div class="empty-state-desc">Pick a snippet from the list or create a new one.</div>
            <div class="empty-state-action">
              <button type="button" class="btn btn-secondary btn-sm" onclick={addSnippet}>
                <Plus size={14} />
                New snippet
              </button>
            </div>
          </div>
        {/if}
      </section>
    </div>
  </ModalShell>
{/if}
