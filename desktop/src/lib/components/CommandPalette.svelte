<script lang="ts">
  import {
    Activity,
    Check,
    Clock,
    Command,
    HardDrive,
    KeyRound,
    Network,
    Palette,
    Play,
    Plus,
    Search,
    Server,
    Settings,
    ShieldCheck,
    Terminal,
    TerminalSquare,
    Wrench,
    X,
  } from "lucide-svelte";
  import type { AppTab, Connection, DesktopSettings } from "$lib/types";

  interface Props {
    open: boolean;
    connections: Connection[];
    activeTab: AppTab;
    settings: DesktopSettings;
    onClose: () => void;
    onSelectTab: (tab: AppTab) => void;
    onConnectHost: (conn: Connection) => void;
    onOpenAddModal: () => void;
    onOpenBatchExec: () => void;
    onPingAll: () => void;
    onFixPermissions: () => void;
    onOpenKeys: () => void;
    onOpenSessionManager: () => void;
    onSelectTheme: (theme: string) => void;
  }

  let {
    open,
    connections,
    activeTab,
    settings,
    onClose,
    onSelectTab,
    onConnectHost,
    onOpenAddModal,
    onOpenBatchExec,
    onPingAll,
    onFixPermissions,
    onOpenKeys,
    onOpenSessionManager,
    onSelectTheme,
  }: Props = $props();

  let query = $state("");
  let selectedIndex = $state(0);
  let inputElement = $state<HTMLInputElement | null>(null);

  interface CommandItem {
    id: string;
    title: string;
    subtitle?: string;
    category: "Hosts" | "Navigation" | "Actions" | "Themes";
    icon: typeof Server;
    badge?: string;
    action: () => void;
  }

  const allItems = $derived.by((): CommandItem[] => {
    const list: CommandItem[] = [];

    // 1. Quick Navigation
    const nav: Array<{ tab: AppTab; label: string; icon: typeof Server }> = [
      { tab: "connections", label: "Go to Hosts", icon: Server },
      { tab: "terminals", label: "Go to Terminals", icon: TerminalSquare },
      { tab: "sftp", label: "Go to SFTP File Browser", icon: HardDrive },
      { tab: "tunnels", label: "Go to Tunnel Studio", icon: Network },
      { tab: "keys", label: "Go to SSH Keys Manager", icon: KeyRound },
      { tab: "audit", label: "Go to Security Auditor", icon: ShieldCheck },
      { tab: "history", label: "Go to Session Logs", icon: Clock },
      { tab: "settings", label: "Go to Settings", icon: Settings },
    ];

    for (const item of nav) {
      list.push({
        id: `nav-${item.tab}`,
        title: item.label,
        category: "Navigation",
        icon: item.icon,
        action: () => {
          onSelectTab(item.tab);
          onClose();
        },
      });
    }

    // 2. Global Actions
    list.push({
      id: "act-new-server",
      title: "New Server Connection",
      subtitle: "Add a new SSH host to configuration",
      category: "Actions",
      icon: Plus,
      badge: "N",
      action: () => {
        onOpenAddModal();
        onClose();
      },
    });

    list.push({
      id: "act-ping-all",
      title: "Ping All Saved Hosts",
      subtitle: "Check reachability and latency for all servers",
      category: "Actions",
      icon: Activity,
      action: () => {
        onPingAll();
        onClose();
      },
    });

    list.push({
      id: "act-batch-exec",
      title: "Run Batch Command",
      subtitle: "Safe multi-host concurrent command execution",
      category: "Actions",
      icon: Terminal,
      action: () => {
        onOpenBatchExec();
        onClose();
      },
    });

    list.push({
      id: "act-fix-perms",
      title: "Fix Insecure Key Permissions",
      subtitle: "Automatically chmod 0600 on SSH keys and 0700 on ~/.ssh",
      category: "Actions",
      icon: Wrench,
      action: () => {
        onFixPermissions();
        onClose();
      },
    });

    list.push({
      id: "act-sessions",
      title: "Manage Active & Detached Sessions",
      subtitle: "View running tabs, pop-outs, and background jobs",
      category: "Actions",
      icon: TerminalSquare,
      action: () => {
        onOpenSessionManager();
        onClose();
      },
    });

    // 3. Themes
    const themes = ["zinc", "cyberpunk", "oled", "slate"];
    for (const t of themes) {
      list.push({
        id: `theme-${t}`,
        title: `Switch Theme to ${t.charAt(0).toUpperCase() + t.slice(1)}`,
        subtitle: settings.theme === t ? "Currently active" : undefined,
        category: "Themes",
        icon: Palette,
        badge: settings.theme === t ? "Active" : undefined,
        action: () => {
          onSelectTheme(t);
          onClose();
        },
      });
    }

    // 4. SSH Hosts (Bayesian ranked)
    for (const conn of connections) {
      list.push({
        id: `host-${conn.id}`,
        title: conn.name,
        subtitle: `${conn.user}@${conn.host}:${conn.port}${conn.tags.length ? ` · #${conn.tags.join(" #")}` : ""}`,
        category: "Hosts",
        icon: Server,
        badge: conn.use_kerberos ? "krb5" : undefined,
        action: () => {
          onConnectHost(conn);
          onClose();
        },
      });
    }

    return list;
  });

  const filteredItems = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      // Default view: Show top hosts, then quick actions, then navigation
      const hosts = allItems.filter((i) => i.category === "Hosts").slice(0, 5);
      const actions = allItems.filter((i) => i.category === "Actions");
      const nav = allItems.filter((i) => i.category === "Navigation").slice(0, 4);
      return [...hosts, ...actions, ...nav];
    }

    return allItems.filter((item) => {
      const matchTitle = item.title.toLowerCase().includes(q);
      const matchSubtitle = item.subtitle?.toLowerCase().includes(q) ?? false;
      const matchCat = item.category.toLowerCase().includes(q);
      return matchTitle || matchSubtitle || matchCat;
    });
  });

  $effect(() => {
    if (open) {
      query = "";
      selectedIndex = 0;
      requestAnimationFrame(() => inputElement?.focus());
    }
  });

  $effect(() => {
    // Keep index bounded
    if (selectedIndex >= filteredItems.length && filteredItems.length > 0) {
      selectedIndex = filteredItems.length - 1;
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = (selectedIndex + 1) % Math.max(1, filteredItems.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = (selectedIndex - 1 + filteredItems.length) % Math.max(1, filteredItems.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (filteredItems[selectedIndex]) {
        filteredItems[selectedIndex].action();
      }
    }
  }
</script>

{#if open}
  <!-- Backdrop -->
  <div
    class="fixed inset-0 z-[200] flex items-start justify-center pt-[12vh] bg-black/70 backdrop-blur-md transition-opacity duration-fast"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) onClose();
    }}
  >
    <!-- Palette Container -->
    <div
      class="w-full max-w-xl rounded-2xl border border-white/10 bg-surface-raised/95 shadow-2xl overflow-hidden flex flex-col max-h-[70vh] animate-in fade-in zoom-in-95 duration-150"
      role="dialog"
      aria-modal="true"
      aria-label="Command Palette"
      tabindex="-1"
      onkeydown={handleKeydown}
    >
      <!-- Search Input Bar -->
      <div class="flex items-center gap-3 px-4 py-3.5 border-b border-border/80 bg-surface-input/50">
        <Search size={18} class="text-accent shrink-0" />
        <input
          bind:this={inputElement}
          type="text"
          bind:value={query}
          placeholder="Search hosts, commands, or jump to view... (Esc to close)"
          class="bg-transparent border-none text-sm text-primary placeholder:text-muted outline-none w-full font-sans"
        />
        {#if query}
          <button
            type="button"
            onclick={() => (query = "")}
            class="text-muted hover:text-primary p-1 rounded-full bg-transparent border-none cursor-pointer"
          >
            <X size={14} />
          </button>
        {/if}
        <span class="kbd text-[10px] text-muted shrink-0">ESC</span>
      </div>

      <!-- Results List -->
      <div class="flex-1 min-h-0 overflow-y-auto p-2 flex flex-col gap-0.5 scrollbar-none">
        {#if filteredItems.length > 0}
          {#each filteredItems as item, index}
            <button
              type="button"
              class="w-full flex items-center justify-between gap-3 px-3 py-2.5 rounded-xl border text-left cursor-pointer transition-all duration-fast
                {selectedIndex === index
                  ? 'border-accent/35 bg-accent/15 text-primary shadow-sm'
                  : 'border-transparent text-secondary bg-transparent hover:bg-surface-hover hover:text-primary'}"
              onmouseenter={() => (selectedIndex = index)}
              onclick={item.action}
            >
              <div class="flex items-center gap-3 min-w-0">
                <div
                  class="w-7 h-7 rounded-lg flex items-center justify-center shrink-0 border
                    {selectedIndex === index
                      ? 'bg-accent text-white border-accent'
                      : 'bg-surface-input border-border text-muted'}"
                >
                  <item.icon size={14} />
                </div>
                <div class="flex flex-col min-w-0">
                  <span class="text-xs font-semibold truncate leading-tight {selectedIndex === index ? 'text-primary' : ''}">
                    {item.title}
                  </span>
                  {#if item.subtitle}
                    <span class="text-[11px] font-mono text-muted truncate leading-tight mt-0.5">
                      {item.subtitle}
                    </span>
                  {/if}
                </div>
              </div>

              <div class="flex items-center gap-2 shrink-0">
                <span class="text-[10px] uppercase font-bold tracking-wider px-1.5 py-0.5 rounded bg-surface border border-border/60 text-muted">
                  {item.category}
                </span>
                {#if item.badge}
                  <span class="badge-pill bg-accent/20 text-accent border border-accent/30">
                    {item.badge}
                  </span>
                {/if}
              </div>
            </button>
          {/each}
        {:else}
          <div class="py-12 flex flex-col items-center justify-center text-muted gap-2">
            <Command size={28} class="opacity-40" />
            <p class="text-xs font-medium">No results found for "{query}"</p>
            <p class="text-[11px] opacity-70">Try searching for host name, user, or action name</p>
          </div>
        {/if}
      </div>

      <!-- Footer Bar -->
      <div class="px-4 py-2 border-t border-border/80 bg-surface-input/30 flex items-center justify-between text-[11px] text-muted">
        <div class="flex items-center gap-3">
          <span class="flex items-center gap-1">
            <span class="kbd text-[9px]">↑</span>
            <span class="kbd text-[9px]">↓</span>
            Navigate
          </span>
          <span class="flex items-center gap-1">
            <span class="kbd text-[9px]">↵</span>
            Select
          </span>
        </div>
        <span class="font-mono text-[10px] text-muted/70">
          {filteredItems.length} available {filteredItems.length === 1 ? 'item' : 'items'}
        </span>
      </div>
    </div>
  </div>
{/if}
