<script lang="ts">
  import {
    Activity,
    Clock,
    HardDrive,
    KeyRound,
    Network,
    Palette,
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
  import { tick } from "svelte";
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

  type Category = "Hosts" | "Actions" | "Navigation" | "Themes";
  const CATEGORY_ORDER: Category[] = ["Hosts", "Actions", "Navigation", "Themes"];

  interface CommandItem {
    id: string;
    title: string;
    /** Secondary text; rendered monospace when `mono` is set (host addresses). */
    subtitle?: string;
    mono?: boolean;
    /** Extra searchable text that is not displayed (tags, aliases). */
    keywords?: string;
    category: Category;
    icon: typeof Server;
    badge?: string;
    /** Keyboard shortcut hint, one entry per key cap. */
    keys?: string[];
    action: () => void;
  }

  interface CommandGroup {
    category: Category;
    start: number;
    items: CommandItem[];
  }

  const THEMES: Array<{ id: string; label: string }> = [
    { id: "zinc", label: "Zinc" },
    { id: "cyberpunk", label: "Midnight" },
    { id: "oled", label: "OLED black" },
    { id: "slate", label: "Slate" },
  ];

  let query = $state("");
  let selectedIndex = $state(0);
  let inputElement = $state<HTMLInputElement | null>(null);
  let listElement = $state<HTMLElement | null>(null);

  function run(fn: () => void) {
    fn();
    onClose();
  }

  const staticItems = $derived.by((): CommandItem[] => {
    const nav: Array<{ tab: AppTab; label: string; icon: typeof Server; key?: string }> = [
      { tab: "connections", label: "Go to hosts", icon: Server, key: "1" },
      { tab: "terminals", label: "Go to terminals", icon: TerminalSquare, key: "2" },
      ...(settings.enable_sftp !== false
        ? [{ tab: "sftp" as AppTab, label: "Go to files", icon: HardDrive }]
        : []),
      ...(settings.enable_tunneling !== false
        ? [{ tab: "tunnels" as AppTab, label: "Go to tunnels", icon: Network }]
        : []),
      { tab: "keys", label: "Go to SSH keys", icon: KeyRound, key: "3" },
      { tab: "audit", label: "Go to security audit", icon: ShieldCheck, key: "4" },
      { tab: "history", label: "Go to history", icon: Clock, key: "5" },
      { tab: "settings", label: "Go to settings", icon: Settings, key: "6" },
    ];

    const actions: CommandItem[] = [
      {
        id: "act-new-server",
        title: "New host",
        subtitle: "Add an SSH host",
        category: "Actions",
        icon: Plus,
        keys: ["N"],
        action: () => run(onOpenAddModal),
      },
      {
        id: "act-batch-exec",
        title: "Run command on hosts",
        subtitle: "Batch execution with dry run",
        keywords: "batch multi exec",
        category: "Actions",
        icon: Terminal,
        action: () => run(onOpenBatchExec),
      },
      {
        id: "act-ping-all",
        title: "Ping all hosts",
        subtitle: "Check reachability and latency",
        category: "Actions",
        icon: Activity,
        action: () => run(onPingAll),
      },
      {
        id: "act-sessions",
        title: "Manage sessions",
        subtitle: "Open tabs, pop-outs and detached sessions",
        category: "Actions",
        icon: TerminalSquare,
        action: () => run(onOpenSessionManager),
      },
      {
        id: "act-fix-perms",
        title: "Fix key permissions",
        subtitle: "chmod 600 on keys, 700 on ~/.ssh",
        category: "Actions",
        icon: Wrench,
        action: () => run(onFixPermissions),
      },
    ];

    const navItems: CommandItem[] = nav.map((item) => ({
      id: `nav-${item.tab}`,
      title: item.label,
      subtitle: activeTab === item.tab ? "Current view" : undefined,
      category: "Navigation",
      icon: item.icon,
      keys: item.key ? [item.key] : undefined,
      action: () => run(item.tab === "keys" ? onOpenKeys : () => onSelectTab(item.tab)),
    }));

    const themeItems: CommandItem[] = THEMES.map((t) => ({
      id: `theme-${t.id}`,
      title: `Theme: ${t.label}`,
      keywords: `switch appearance ${t.id}`,
      category: "Themes",
      icon: Palette,
      badge: settings.theme === t.id ? "Active" : undefined,
      action: () => run(() => onSelectTheme(t.id)),
    }));

    return [...actions, ...navItems, ...themeItems];
  });

  const hostItems = $derived(
    connections.map(
      (conn): CommandItem => ({
        id: `host-${conn.id}`,
        title: conn.name,
        subtitle: `${conn.user}@${conn.host}${conn.port !== 22 ? `:${conn.port}` : ""}`,
        mono: true,
        keywords: conn.tags.join(" "),
        category: "Hosts",
        icon: Server,
        badge: conn.use_kerberos ? "krb5" : undefined,
        action: () => run(() => onConnectHost(conn)),
      }),
    ),
  );

  const groups = $derived.by((): CommandGroup[] => {
    const q = query.trim().toLowerCase();
    const buckets: Record<Category, CommandItem[]> = { Hosts: [], Actions: [], Navigation: [], Themes: [] };

    if (!q) {
      // Default view: top-ranked hosts, then actions and navigation.
      buckets.Hosts = hostItems.slice(0, 5);
      for (const item of staticItems) {
        if (item.category !== "Themes") buckets[item.category].push(item);
      }
    } else {
      for (const item of [...hostItems, ...staticItems]) {
        if (
          item.title.toLowerCase().includes(q) ||
          item.subtitle?.toLowerCase().includes(q) ||
          item.keywords?.toLowerCase().includes(q) ||
          item.category.toLowerCase().includes(q)
        ) {
          buckets[item.category].push(item);
        }
      }
    }

    const out: CommandGroup[] = [];
    let start = 0;
    for (const category of CATEGORY_ORDER) {
      const items = buckets[category];
      if (items.length === 0) continue;
      out.push({ category, start, items });
      start += items.length;
    }
    return out;
  });

  const flatItems = $derived(groups.flatMap((g) => g.items));

  let previousFocus: HTMLElement | null = null;

  $effect(() => {
    if (!open) return;
    previousFocus = document.activeElement as HTMLElement | null;
    query = "";
    selectedIndex = 0;
    const raf = requestAnimationFrame(() => inputElement?.focus());
    return () => {
      cancelAnimationFrame(raf);
      previousFocus?.focus();
    };
  });

  async function select(index: number) {
    selectedIndex = index;
    await tick();
    listElement
      ?.querySelector<HTMLElement>(`[data-index="${index}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }

  function handleKeydown(e: KeyboardEvent) {
    const count = flatItems.length;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown" && count > 0) {
      e.preventDefault();
      void select((selectedIndex + 1) % count);
    } else if (e.key === "ArrowUp" && count > 0) {
      e.preventDefault();
      void select((selectedIndex - 1 + count) % count);
    } else if (e.key === "Enter") {
      e.preventDefault();
      flatItems[Math.min(selectedIndex, count - 1)]?.action();
    }
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-[200] flex items-start justify-center bg-overlay px-4 pt-[15vh]"
    style="animation: fade-in 120ms ease-out"
    role="presentation"
    onpointerdown={(e) => {
      if (e.target === e.currentTarget) onClose();
    }}
  >
    <div
      class="flex max-h-[min(560px,72vh)] w-full max-w-[640px] flex-col overflow-hidden rounded-xl border border-border-hover bg-surface-raised shadow-xl"
      style="animation: popover-enter 140ms var(--ease-out)"
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
      tabindex="-1"
      onkeydown={handleKeydown}
    >
      <div class="flex h-13 shrink-0 items-center gap-3 border-b border-border px-4">
        <Search size={16} class="shrink-0 text-muted" />
        <input
          bind:this={inputElement}
          type="text"
          bind:value={query}
          oninput={() => (selectedIndex = 0)}
          placeholder="Search hosts, actions and views"
          aria-label="Search commands"
          role="combobox"
          aria-expanded="true"
          aria-controls="command-palette-list"
          aria-activedescendant={flatItems[selectedIndex] ? `cmd-${flatItems[selectedIndex].id}` : undefined}
          autocomplete="off"
          spellcheck="false"
          class="h-full min-w-0 flex-1 border-none bg-transparent text-base text-primary outline-none placeholder:text-muted"
        />
        {#if query}
          <button
            type="button"
            class="btn-icon btn-icon-sm"
            aria-label="Clear search"
            onclick={() => {
              query = "";
              selectedIndex = 0;
              inputElement?.focus();
            }}
          >
            <X size={14} />
          </button>
        {/if}
      </div>

      <div
        bind:this={listElement}
        id="command-palette-list"
        class="min-h-0 flex-1 overflow-y-auto p-1.5"
        role="listbox"
        aria-label="Commands"
      >
        {#each groups as group (group.category)}
          <div role="group" aria-label={group.category} class="pb-1">
            <div class="section-label px-2 pb-1 pt-2">{group.category}</div>
            {#each group.items as item, i (item.id)}
              {@const index = group.start + i}
              {@const active = index === selectedIndex}
              <button
                type="button"
                id="cmd-{item.id}"
                data-index={index}
                role="option"
                aria-selected={active}
                tabindex="-1"
                class="flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-md px-2 text-left text-sm transition-colors duration-fast {active
                  ? 'bg-surface-active text-primary'
                  : 'text-secondary'}"
                onmousemove={() => {
                  if (selectedIndex !== index) selectedIndex = index;
                }}
                onclick={item.action}
              >
                <item.icon size={14} class="shrink-0 {active ? 'text-primary' : 'text-muted'}" />
                <span class="shrink-0 truncate {active ? 'text-primary' : 'text-primary/90'}">{item.title}</span>
                {#if item.subtitle}
                  <span class="min-w-0 truncate text-xs text-muted {item.mono ? 'font-mono' : ''}">{item.subtitle}</span>
                {/if}
                <span class="ml-auto flex shrink-0 items-center gap-1.5 pl-2">
                  {#if item.badge}
                    <span class="badge badge-neutral">{item.badge}</span>
                  {/if}
                  {#if item.category === "Hosts" && active}
                    <span class="text-xs text-muted">Connect</span>
                    <kbd class="kbd">↵</kbd>
                  {:else if item.keys}
                    {#each item.keys as key (key)}
                      <kbd class="kbd">{key}</kbd>
                    {/each}
                  {/if}
                </span>
              </button>
            {/each}
          </div>
        {:else}
          <div class="flex flex-col items-center gap-1 px-6 py-10 text-center">
            <p class="m-0 text-sm font-medium text-primary">No results for “{query.trim()}”</p>
            <p class="m-0 text-xs text-muted">Try a host name, address, tag or action.</p>
          </div>
        {/each}
      </div>

      <div class="flex h-9 shrink-0 items-center gap-4 border-t border-border px-3 text-xs text-muted">
        <span class="flex items-center gap-1.5">
          <kbd class="kbd">↑</kbd><kbd class="kbd">↓</kbd> Navigate
        </span>
        <span class="flex items-center gap-1.5"><kbd class="kbd">↵</kbd> Open</span>
        <span class="flex items-center gap-1.5"><kbd class="kbd">esc</kbd> Close</span>
        <span class="ml-auto tabular-nums">{flatItems.length} {flatItems.length === 1 ? "result" : "results"}</span>
      </div>
    </div>
  </div>
{/if}
