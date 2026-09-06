<script lang="ts">
  import {
    Check,
    Code,
    Copy,
    Play,
    Plus,
    Search,
    Sparkles,
    Tag,
    Terminal,
    Trash2,
    X,
  } from "lucide-svelte";
  import { notify } from "$lib/stores/notifications.svelte";
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

  let searchQuery = $state("");
  let selectedCategory = $state("All");
  let copiedId = $state<string | null>(null);

  let snippets = $state<SnippetItem[]>([
    {
      id: "1",
      title: "Tail System Error Logs",
      command: "tail -n 100 -f /var/log/syslog | grep -i error",
      category: "Logs",
      description: "Monitor real-time system syslog entries filtered by error severity",
    },
    {
      id: "2",
      title: "Disk Usage & Top Directories",
      command: "df -h && echo '--- Top Directories ---' && du -sh * 2>/dev/null | sort -rh | head -n 10",
      category: "System",
      description: "Display mounted disk usage and list top 10 largest folders in current path",
    },
    {
      id: "3",
      title: "Docker Container Live Stream",
      command: "docker logs --tail 100 -f {{container_name}}",
      category: "Docker",
      description: "Stream live logs from specified Docker container instance",
    },
    {
      id: "4",
      title: "Nginx Service Health & Errors",
      command: "systemctl status nginx --no-pager && tail -n 20 /var/log/nginx/error.log",
      category: "Services",
      description: "Inspect Nginx service status and output last 20 error log entries",
    },
    {
      id: "5",
      title: "Listening Ports & Socket PID",
      command: "ss -tulpn | grep LISTEN",
      category: "Network",
      description: "Identify all active listening TCP/UDP ports and associated process PIDs",
    },
    {
      id: "6",
      title: "Top CPU & Memory Processes",
      command: "ps aux --sort=-%cpu | head -n 10",
      category: "System",
      description: "Rank processes by CPU utilization percentage",
    },
  ]);

  const categories = $derived.by(() => {
    const set = new Set<string>(["All"]);
    snippets.forEach((s) => set.add(s.category));
    return Array.from(set);
  });

  const filteredSnippets = $derived.by(() => {
    return snippets.filter((s) => {
      const matchCat = selectedCategory === "All" || s.category === selectedCategory;
      const matchQuery =
        !searchQuery.trim() ||
        s.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
        s.command.toLowerCase().includes(searchQuery.toLowerCase()) ||
        (s.description && s.description.toLowerCase().includes(searchQuery.toLowerCase()));
      return matchCat && matchQuery;
    });
  });

  function copySnippet(s: SnippetItem) {
    navigator.clipboard.writeText(s.command).then(() => {
      copiedId = s.id;
      setTimeout(() => (copiedId = null), 2000);
      notify("Command snippet copied to clipboard", "info");
    });
  }

  function handleRun(s: SnippetItem) {
    onRunSnippet(s.command);
    onClose();
  }
</script>

{#if show}
  <ModalShell
    open={show}
    title="Command Snippets & Automation Library"
    onClose={onClose}
    closeOnBackdrop={false}
    width="lg"
    panelClass="p-6 gap-4 max-h-[85vh] relative overflow-hidden"
  >
    <!-- Header -->
    <div class="flex items-center justify-between gap-4 pb-3 border-b border-border">
      <div class="flex items-center gap-3">
        <div class="icon-tile rounded-md">
          <Code size={16} />
        </div>
        <div>
          <h3 class="text-sm font-bold text-primary m-0 flex items-center gap-2">
            <span>Snippets & Automation Library</span>
            <span class="badge badge-subtle">{filteredSnippets.length} ready</span>
          </h3>
          <p class="text-xs text-muted mt-0.5 m-0">
            One-click sysadmin & DevOps command automation templates
          </p>
        </div>
      </div>

        <button
          type="button"
          class="p-1.5 rounded-lg text-muted hover:text-primary hover:bg-white/10 transition-colors"
          onclick={onClose}
        >
          <X size={16} />
        </button>
      </div>

      <!-- Search Header & Category Filter Tabs -->
      <div class="flex flex-col sm:flex-row items-center gap-3">
        <div class="relative flex-1 w-full">
          <Search size={14} class="absolute left-3 top-2.5 text-muted" />
          <input
            type="text"
            placeholder="Search automation templates (Ctrl+/)..."
            bind:value={searchQuery}
            class="w-full bg-surface-input border border-border text-primary py-1.5 pl-8 pr-3 rounded-lg outline-none text-xs font-medium transition-all focus:border-border-focus"
          />
        </div>

        <div class="flex items-center gap-1.5 overflow-x-auto w-full sm:w-auto scrollbar-none">
          {#each categories as cat (cat)}
            <button
              type="button"
              class="filter-chip {selectedCategory === cat ? 'filter-chip-active' : 'filter-chip-idle'}"
              onclick={() => (selectedCategory = cat)}
            >
              {cat}
            </button>
          {/each}
        </div>
      </div>

      <!-- Snippets Cards Stream -->
      <div class="flex-1 overflow-y-auto flex flex-col gap-3 pr-1">
        {#each filteredSnippets as s (s.id)}
          <div class="panel p-4 flex flex-col gap-3">
            <div class="flex items-center justify-between gap-2">
              <div class="flex items-center gap-2">
                <Terminal size={14} class="text-accent shrink-0" />
                <span class="text-xs font-semibold text-primary">{s.title}</span>
              </div>
              <span class="tag text-[10px]">
                {s.category}
              </span>
            </div>

            {#if s.description}
              <p class="text-[11px] text-muted m-0">{s.description}</p>
            {/if}

            <!-- Code Block with Syntax Accent -->
            <pre class="bg-surface-terminal p-3 rounded-lg border border-border text-secondary font-mono text-[11px] overflow-x-auto m-0 select-all leading-relaxed">{s.command}</pre>

            <div class="flex items-center justify-end gap-2 pt-1 border-t border-border/50">
              <button
                type="button"
                class="btn btn-secondary btn-sm"
                onclick={() => copySnippet(s)}
              >
                {#if copiedId === s.id}
                  <Check size={12} class="text-running" />
                  <span>Copied!</span>
                {:else}
                  <Copy size={12} />
                  <span>Copy</span>
                {/if}
              </button>
              <button
                type="button"
                class="btn btn-primary btn-sm"
                onclick={() => handleRun(s)}
              >
                <Play size={12} />
                <span>Run in Terminal</span>
              </button>
            </div>
          </div>
        {/each}
      </div>
  </ModalShell>
{/if}
