<script lang="ts">
  import {
    Activity,
    FolderOpen,
    Lock,
    LockOpen,
    Palette,
    Pencil,
    Plus,
    RefreshCw,
    Search,
    Server,
    Settings,
    Terminal,
    TerminalSquare,
    Upload,
    UserPlus,
    Users,
    Wrench,
    X,
    Copy,
    ShieldAlert,
  } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";
  import type {
    AppTab,
    Connection,
    DesktopSettings,
    Prediction,
    TeamAttention,
    TeamSyncResult,
    TeamsStatus,
  } from "$lib/types";
  import { predictionReason } from "$lib/utils/prediction";
  import { GLOBAL_SHORTCUTS, HOST_SHORTCUTS, MOD, VIEW_ORDER, viewDigit, type Shortcut } from "$lib/shortcuts";
  import { VIEW_META, viewEnabled } from "$lib/viewMeta";
  import { SETTINGS_SECTIONS, requestSettingsSection } from "$lib/stores/settingsNav.svelte";
  import { requestTeamFocus } from "$lib/stores/attention.svelte";
  import { requestFilesHost } from "$lib/stores/filesHost.svelte";
  import { TEAMS_CHANGED_EVENT } from "$lib/stores/teamsAttention.svelte";
  import { THEME_OPTIONS } from "$lib/utils/theme";
  import { toSshCommand } from "$lib/utils/sshCommand";
  import { notify } from "$lib/stores/notifications.svelte";

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
    onOpenUpload: () => void;
    onPingAll: () => void;
    onFixPermissions: () => void;
    onOpenKeys: () => void;
    onOpenSessionManager: () => void;
    onSelectTheme: (theme: string) => void;
    onEditHost: (conn: Connection) => void;
    onCopyCommand: (text: string, id: string) => void;
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
    onOpenUpload,
    onPingAll,
    onFixPermissions,
    onOpenKeys,
    onOpenSessionManager,
    onSelectTheme,
    onEditHost,
    onCopyCommand,
  }: Props = $props();

  type Category = "Suggested" | "Hosts" | "Actions" | "Teams" | "Navigation" | "Settings" | "Themes";
  const CATEGORY_ORDER: Category[] = ["Suggested", "Hosts", "Actions", "Teams", "Navigation", "Settings", "Themes"];

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
    /** Shown in the default (empty query) view even when its category is otherwise search-only. */
    urgent?: boolean;
    /** Keyboard shortcut hint, one entry per key cap. */
    keys?: string[];
    action: () => void;
  }

  interface CommandGroup {
    category: Category;
    start: number;
    items: CommandItem[];
  }

  const shortcut = (list: Shortcut[], id: string) => list.find((s) => s.id === id);
  /** First combo of a registry entry, as key caps. */
  const keysOf = (list: Shortcut[], id: string) => shortcut(list, id)?.combos[0];

  let query = $state("");
  let selectedIndex = $state(0);
  let inputElement = $state<HTMLInputElement | null>(null);
  let listElement = $state<HTMLElement | null>(null);
  let suggestions = $state.raw<Prediction[]>([]);
  /** Teams state read when the palette opens; null until known or when unavailable. */
  let teamsInfo = $state.raw<{
    identity: boolean;
    unlocked: boolean;
    teams: { id: string; name: string; admin: boolean; pending: number }[];
  } | null>(null);

  function run(fn: () => void) {
    fn();
    onClose();
  }

  function openTeams(teamId?: string) {
    if (teamId) requestTeamFocus(teamId);
    onSelectTab("teams");
  }

  async function syncAllTeams() {
    try {
      const results = await invoke<TeamSyncResult[]>("teams_sync", { team: null });
      const ok = results.filter((r) => r.result).length;
      for (const r of results) if (r.error) notify(`${r.team}: ${r.error}`, "error");
      if (ok) notify(`Synced ${ok} ${ok === 1 ? "team" : "teams"}`, "success");
    } catch (e) {
      notify(`Sync failed: ${e}`, "error");
    }
    window.dispatchEvent(new Event(TEAMS_CHANGED_EVENT));
  }

  async function lockTeams() {
    try {
      await invoke("teams_lock");
      notify("Teams locked", "success");
    } catch (e) {
      notify(`Lock failed: ${e}`, "error");
    }
    window.dispatchEvent(new Event(TEAMS_CHANGED_EVENT));
  }

  /**
   * Teams verbs that make sense right now. Nothing here grants a key: the
   * "review" verbs only open the team, where an admin checks the device's
   * fingerprint out of band.
   */
  function teamVerbs(): CommandItem[] {
    const info = teamsInfo;
    const base = (id: string, title: string, icon: typeof Server, action: () => void, extra: Partial<CommandItem> = {}): CommandItem => ({
      id: `teams-${id}`,
      title,
      category: "Teams",
      icon,
      keywords: "team teams",
      action: () => run(action),
      ...extra,
    });
    // "Go to Teams" is already in Navigation.
    const items: ReturnType<typeof base>[] = [];
    if (!info?.identity) return items;
    if (!info.unlocked) {
      items.push(base("unlock", "Unlock Teams…", LockOpen, () => openTeams(), { subtitle: "Asks for your passphrase in Teams" }));
      return items;
    }
    if (info.teams.length > 0) {
      items.push(base("sync", "Sync all teams", RefreshCw, () => void syncAllTeams(), { subtitle: "bssh teams sync" }));
    }
    items.push(base("lock", "Lock Teams", Lock, () => void lockTeams(), { subtitle: "bssh teams lock" }));
    for (const t of info.teams) {
      if (t.pending > 0) {
        items.push(
          base(`review-${t.id}`, `Review waiting devices in ${t.name}`, ShieldAlert, () => openTeams(t.id), {
            subtitle: `${t.pending} ${t.pending === 1 ? "device" : "devices"} · check fingerprints before granting`,
            urgent: true,
          }),
        );
      }
      if (t.admin) {
        items.push(base(`invite-${t.id}`, `Invite to ${t.name}…`, UserPlus, () => openTeams(t.id), { keywords: "team teams invite member add" }));
      }
    }
    return items;
  }

  const staticItems = $derived.by((): CommandItem[] => {
    const nav = VIEW_ORDER.filter((tab) => viewEnabled(tab, settings)).map((tab) => ({
      tab,
      label: `Go to ${VIEW_META[tab as Exclude<AppTab, "settings">].label}`,
      icon: VIEW_META[tab as Exclude<AppTab, "settings">].icon,
      key: viewDigit(tab),
    }));
    nav.push({ tab: "settings", label: "Go to Settings", icon: Settings, key: null });

    const actions: CommandItem[] = [
      {
        id: "act-new-server",
        title: "New host",
        subtitle: "Add an SSH host",
        category: "Actions",
        icon: Plus,
        keys: keysOf(GLOBAL_SHORTCUTS, "new-host"),
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
        id: "act-upload-files",
        title: "Upload files…",
        subtitle: "Copy files or folders to several hosts",
        keywords: "upload scp sftp transfer multi",
        category: "Actions",
        icon: Upload,
        action: () => run(onOpenUpload),
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
      keys: item.tab === "settings" ? keysOf(GLOBAL_SHORTCUTS, "settings") : item.key ? [item.key] : undefined,
      action: () => run(item.tab === "keys" ? onOpenKeys : () => onSelectTab(item.tab)),
    }));

    const teamItems = teamVerbs();

    const themeItems: CommandItem[] = THEME_OPTIONS.map((t) => ({
      id: `theme-${t.id}`,
      title: `Theme: ${t.label}`,
      keywords: `switch appearance ${t.id}`,
      category: "Themes",
      icon: Palette,
      badge: settings.theme === t.id ? "Active" : undefined,
      action: () => run(() => onSelectTheme(t.id)),
    }));

    const settingsItems: CommandItem[] = SETTINGS_SECTIONS.map((section) => ({
      id: `settings-${section.id}`,
      title: `Settings: ${section.label}`,
      category: "Settings",
      icon: Settings,
      action: () =>
        run(() => {
          requestSettingsSection(section.id);
          onSelectTab("settings");
        }),
    }));

    return [...actions, ...teamItems, ...navItems, ...settingsItems, ...themeItems];
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

  const suggestedItems = $derived(
    suggestions.map(
      (pred): CommandItem => ({
        id: `suggested-${pred.connection.id}`,
        title: pred.connection.name,
        subtitle: predictionReason(pred),
        category: "Suggested",
        icon: Server,
        badge: pred.connection.use_kerberos ? "krb5" : undefined,
        action: () => run(() => onConnectHost(pred.connection)),
      }),
    ),
  );

  const groups = $derived.by((): CommandGroup[] => {
    const q = query.trim().toLowerCase();
    const buckets: Record<Category, CommandItem[]> = {
      Suggested: [],
      Hosts: [],
      Actions: [],
      Teams: [],
      Navigation: [],
      Settings: [],
      Themes: [],
    };

    if (!q) {
      // Default view: suggested hosts, top-ranked hosts, then actions and navigation.
      buckets.Suggested = suggestedItems;
      buckets.Hosts = hostItems.slice(0, 5);
      for (const item of staticItems) {
        if (item.category === "Themes" || item.category === "Settings") continue;
        if (item.category === "Teams" && !item.urgent) continue;
        buckets[item.category].push(item);
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

  /** The host under the highlight (a Hosts or Suggested row), if any. */
  const highlightedHost = $derived.by((): Connection | null => {
    const item = flatItems[Math.min(selectedIndex, flatItems.length - 1)];
    if (!item || (item.category !== "Hosts" && item.category !== "Suggested")) return null;
    return connections.find((c) => item.id === `host-${c.id}` || item.id === `suggested-${c.id}`) ?? null;
  });

  interface HostVerb {
    id: string;
    label: string;
    icon: typeof Server;
    keys: string[];
    cli?: string;
    run: (conn: Connection) => void;
  }

  const HOST_VERBS: HostVerb[] = ([
    // Keys and CLI come from the registry below.
    { id: "host-connect", label: "Connect", icon: Terminal, run: (c) => onConnectHost(c) },
    { id: "host-connect-tab", label: "Connect in new tab", icon: TerminalSquare, run: (c) => onConnectHost(c) },
    { id: "host-files", label: "Browse files", icon: FolderOpen, run: (c) => {
        requestFilesHost(c.id);
        onSelectTab("sftp");
      } },
    {
      id: "host-copy",
      label: "Copy ssh command",
      icon: Copy,
      run: (c) => onCopyCommand(toSshCommand(c), c.id),
    },
    { id: "host-edit", label: "Edit", icon: Pencil, run: (c) => onEditHost(c) },
  ] as Omit<HostVerb, "keys" | "cli">[]).map((v) => {
    const sc = shortcut(HOST_SHORTCUTS, v.id);
    return { ...v, keys: sc?.combos[0] ?? [], cli: sc?.cli };
  });

  const visibleVerbs = $derived(HOST_VERBS.filter((v) => v.id !== "host-files" || viewEnabled("sftp", settings)));
  let hoveredVerb = $state<string | null>(null);
  const verbCli = $derived.by(() => {
    const verb = visibleVerbs.find((v) => v.id === hoveredVerb) ?? visibleVerbs[0];
    return verb?.cli && highlightedHost ? verb.cli.replace("<host>", highlightedHost.name) : null;
  });

  function runVerb(verb: HostVerb) {
    const host = highlightedHost;
    if (!host) return;
    run(() => verb.run(host));
  }

  let previousFocus: HTMLElement | null = null;

  $effect(() => {
    if (!open) return;
    previousFocus = document.activeElement as HTMLElement | null;
    query = "";
    selectedIndex = 0;
    let cancelled = false;
    invoke<Prediction[]>("predict_connections", { limit: 3 })
      .then((list) => {
        if (!cancelled) {
          suggestions = list.filter((p) => p.breakdown.total_w > 0 || Boolean(p.connection.last_used));
        }
      })
      .catch(() => {
        if (!cancelled) suggestions = [];
      });
    teamsInfo = null;
    invoke<TeamsStatus>("teams_status")
      .then(async (status) => {
        if (cancelled) return;
        const identity = Boolean(status?.identity);
        const unlocked = Boolean(status?.unlocked);
        let attention: TeamAttention[] = [];
        if (unlocked) attention = (await invoke<TeamAttention[] | null>("teams_attention").catch(() => null)) ?? [];
        if (cancelled) return;
        teamsInfo = {
          identity,
          unlocked,
          teams: (status?.teams ?? []).map((t) => {
            const a = attention.find((x) => x.team_id === t.team_id);
            return { id: t.team_id, name: t.name, admin: a?.role === "admin", pending: a?.pending_devices ?? 0 };
          }),
        };
      })
      .catch(() => {
        if (!cancelled) teamsInfo = null;
      });
    const raf = requestAnimationFrame(() => inputElement?.focus());
    return () => {
      cancelled = true;
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
      const combo = (id: string) => {
        const k = keysOf(HOST_SHORTCUTS, id) ?? [];
        return k.includes("Shift") ? e.shiftKey : k.includes(MOD) ? e.ctrlKey || e.metaKey : false;
      };
      if (highlightedHost && combo("host-connect-tab")) {
        runVerb(HOST_VERBS.find((v) => v.id === "host-connect-tab")!);
      } else if (highlightedHost && combo("host-files") && viewEnabled("sftp", settings)) {
        runVerb(HOST_VERBS.find((v) => v.id === "host-files")!);
      } else {
        flatItems[Math.min(selectedIndex, count - 1)]?.action();
      }
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
                  {#if (item.category === "Hosts" || item.category === "Suggested") && active}
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

      {#if highlightedHost}
        <div class="shrink-0 border-t border-border px-3 py-2" role="group" aria-label="Actions for {highlightedHost.name}">
          <div class="flex flex-wrap items-center gap-1.5">
            {#each visibleVerbs as verb (verb.id)}
              <button
                type="button"
                class="btn btn-secondary btn-sm gap-1.5"
                onclick={() => runVerb(verb)}
                onmouseenter={() => (hoveredVerb = verb.id)}
                onfocus={() => (hoveredVerb = verb.id)}
                onmouseleave={() => (hoveredVerb = null)}
                onblur={() => (hoveredVerb = null)}
              >
                <verb.icon size={13} />
                {verb.label}
                {#each verb.keys as key (key)}<kbd class="kbd">{key}</kbd>{/each}
              </button>
            {/each}
          </div>
          {#if verbCli}
            <p class="m-0 mt-1.5 truncate font-mono text-xs text-muted">{verbCli}</p>
          {/if}
        </div>
      {/if}

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
