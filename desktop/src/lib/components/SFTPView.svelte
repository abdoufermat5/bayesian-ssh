<script lang="ts">
  import {
    ArrowUp,
    Check,
    ChevronRight,
    Copy,
    Eye,
    EyeOff,
    File,
    FileArchive,
    FileCode,
    FileSymlink,
    FileText,
    Folder,
    HardDrive,
    Image,
    LayoutGrid,
    List,
    Pencil,
    RefreshCw,
    Search,
    X,
  } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Connection } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { appState } from "$lib/stores/appState.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";

  export interface RemoteFileEntry {
    name: string;
    path: string;
    is_dir: boolean;
    size: number;
    permissions: string;
    modified: string;
  }

  interface Props {
    connections: Connection[];
  }

  let { connections }: Props = $props();

  let selectedConnectionId = $state<string | null>(null);
  let currentPath = $state<string>(appState.settings.sftp_default_remote_path || "/");
  let pathInput = $state<string>("");
  let editingPath = $state(false);
  let filterQuery = $state<string>("");
  let viewMode = $state<"list" | "grid">("list");
  let showHidden = $state<boolean>(appState.settings.sftp_show_hidden_files !== false);
  let entries = $state.raw<RemoteFileEntry[]>([]);
  let isConnected = $state<boolean>(false);
  let loading = $state<boolean>(false);
  let errorMsg = $state<string | null>(null);
  let copiedPath = $state<string | null>(null);
  // Bumped per listing request (and on host switch / disconnect) so a slow
  // response for an older path or another host never overwrites newer state.
  let loadSeq = 0;

  const selectedConnection = $derived.by(() => {
    if (selectedConnectionId) {
      return connections.find((c) => c.id === selectedConnectionId) ?? connections[0] ?? null;
    }
    return connections.length > 0 ? connections[0] : null;
  });

  const hostOptions = $derived(
    connections.map((c) => ({ value: c.id, label: `${c.name} (${c.user}@${c.host})` })),
  );

  const quickBookmarks = [
    { label: "Root", path: "/" },
    { label: "Home", path: "~" },
    { label: "Web", path: "/var/www" },
    { label: "Config", path: "/etc" },
    { label: "Logs", path: "/var/log" },
    { label: "Temp", path: "/tmp" },
  ];

  /** Leading segment that cannot be navigated above ("/", "~" or "."). */
  function pathRoot(path: string): string {
    if (path === "~" || path.startsWith("~/")) return "~";
    if (path.startsWith("/")) return "/";
    return ".";
  }

  function parentOf(path: string): string | null {
    const root = pathRoot(path);
    const rest = (root === "/" ? path : path.slice(root.length)).split("/").filter(Boolean);
    if (rest.length === 0) return null;
    rest.pop();
    if (rest.length === 0) return root;
    return root === "/" ? "/" + rest.join("/") : `${root}/${rest.join("/")}`;
  }

  const pathBreadcrumbs = $derived.by(() => {
    const root = pathRoot(currentPath);
    const crumbs = [{ label: root === "/" ? "/" : root, path: root }];
    const rest = (root === "/" ? currentPath : currentPath.slice(root.length)).split("/").filter(Boolean);
    let acc = root === "/" ? "" : root;
    for (const part of rest) {
      acc += "/" + part;
      crumbs.push({ label: part, path: acc });
    }
    return crumbs;
  });

  const canGoUp = $derived(parentOf(currentPath) !== null);

  const hiddenCount = $derived(entries.reduce((n, e) => n + (e.name.startsWith(".") ? 1 : 0), 0));
  const visibleEntries = $derived(
    showHidden || hiddenCount === 0 ? entries : entries.filter((e) => !e.name.startsWith(".")),
  );

  const filteredEntries = $derived.by(() => {
    const query = filterQuery.trim().toLowerCase();
    if (!query) return visibleEntries;
    return visibleEntries.filter(
      (entry) =>
        entry.name.toLowerCase().includes(query) ||
        entry.permissions.toLowerCase().includes(query),
    );
  });

  const summary = $derived.by(() => {
    let dirs = 0;
    let files = 0;
    let totalSize = 0;
    for (const e of visibleEntries) {
      if (e.is_dir) dirs += 1;
      else {
        files += 1;
        totalSize += e.size;
      }
    }
    return { dirs, files, totalSize };
  });

  async function loadDirectory(path: string) {
    if (!selectedConnection) return;
    const seq = ++loadSeq;
    loading = true;
    errorMsg = null;
    editingPath = false;
    try {
      const res = await invoke<RemoteFileEntry[]>("list_remote_directory", {
        connectionName: selectedConnection.name,
        remotePath: path,
      });
      if (seq !== loadSeq) return;
      entries = res;
      currentPath = path;
      isConnected = true;
    } catch (err: unknown) {
      if (seq !== loadSeq) return;
      errorMsg = String(err);
      notify(`SFTP error: ${err}`, "error");
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  function resetSession() {
    loadSeq += 1;
    loading = false;
    isConnected = false;
    editingPath = false;
    entries = [];
  }

  function handleConnectClick() {
    if (isConnected) {
      resetSession();
      errorMsg = null;
      notify("Disconnected from SFTP session", "info");
    } else {
      loadDirectory(currentPath);
    }
  }

  function startEditingPath() {
    pathInput = currentPath;
    editingPath = true;
  }

  function goToPath() {
    const nextPath = pathInput.trim() || "/";
    loadDirectory(nextPath);
  }

  function navigateUp() {
    const parent = parentOf(currentPath);
    if (parent !== null) loadDirectory(parent);
  }

  function handleEntryClick(entry: RemoteFileEntry) {
    if (entry.is_dir) {
      loadDirectory(entry.path);
    } else {
      copyPath(entry.path);
    }
  }

  function copyPath(p: string) {
    copyTextWithFallback(p);
    copiedPath = p;
    notify(`Copied remote path: ${p}`, "success");
    setTimeout(() => {
      if (copiedPath === p) copiedPath = null;
    }, 2000);
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.min(sizes.length - 1, Math.floor(Math.log(bytes) / Math.log(k)));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  const CODE_EXT = new Set(["js", "ts", "py", "rs", "go", "json", "yaml", "yml", "toml", "html", "css", "sh", "conf", "ini"]);
  const ARCHIVE_EXT = new Set(["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "zst"]);
  const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "svg", "webp", "gif"]);
  const TEXT_EXT = new Set(["txt", "md", "log", "csv"]);

  function getFileIcon(entry: RemoteFileEntry) {
    if (entry.is_dir) return Folder;
    if (entry.permissions.startsWith("l")) return FileSymlink;
    const ext = entry.name.includes(".") ? entry.name.split(".").pop()!.toLowerCase() : "";
    if (CODE_EXT.has(ext)) return FileCode;
    if (ARCHIVE_EXT.has(ext)) return FileArchive;
    if (IMAGE_EXT.has(ext)) return Image;
    if (TEXT_EXT.has(ext)) return FileText;
    return File;
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

{#snippet copyButton(entry: RemoteFileEntry)}
  <button
    type="button"
    class="btn-icon btn-icon-sm"
    onclick={(e) => {
      e.stopPropagation();
      copyPath(entry.path);
    }}
    aria-label="Copy remote path"
    title="Copy remote path"
  >
    {#if copiedPath === entry.path}
      <Check size={14} class="text-success" />
    {:else}
      <Copy size={14} />
    {/if}
  </button>
{/snippet}

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Files</h1>
      {#if isConnected && selectedConnection}
        <span class="flex min-w-0 items-center gap-1.5 text-sm text-muted">
          <span class="status-dot status-dot-sm status-dot-success self-center" aria-hidden="true"></span>
          <span class="truncate font-mono text-xs">{selectedConnection.user}@{selectedConnection.host}</span>
        </span>
      {:else}
        <span class="text-sm text-muted">Not connected</span>
      {/if}
    </div>
    <div class="view-actions">
      <div class="w-72">
        <CustomSelect
          label="Host"
          options={hostOptions}
          value={selectedConnection?.id ?? ""}
          placeholder="No hosts"
          disabled={connections.length === 0}
          onChange={(val) => {
            selectedConnectionId = val;
            resetSession();
            errorMsg = null;
          }}
        />
      </div>
      {#if isConnected}
        <button type="button" class="btn btn-secondary" onclick={handleConnectClick}>
          Disconnect
        </button>
      {:else}
        <button
          type="button"
          class="btn btn-primary"
          onclick={handleConnectClick}
          disabled={!selectedConnection || loading}
        >
          {#if loading}
            <span class="spinner size-3"></span>
            Connecting
          {:else}
            Connect
          {/if}
        </button>
      {/if}
    </div>
  </header>

  {#if isConnected}
    <div class="view-toolbar flex-nowrap">
      <button
        type="button"
        class="btn-icon"
        onclick={navigateUp}
        disabled={!canGoUp || loading}
        aria-label="Parent directory"
        title="Parent directory"
      >
        <ArrowUp size={14} />
      </button>
      <button
        type="button"
        class="btn-icon"
        onclick={() => loadDirectory(currentPath)}
        disabled={loading}
        aria-label="Refresh"
        title="Refresh"
      >
        <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
      </button>

      {#if editingPath}
        <form
          class="flex min-w-0 flex-1"
          onsubmit={(event) => {
            event.preventDefault();
            goToPath();
          }}
        >
          <input
            type="text"
            class="input input-mono"
            bind:value={pathInput}
            aria-label="Remote path"
            spellcheck="false"
            autocomplete="off"
            use:focusOnMount
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                editingPath = false;
              }
            }}
            onblur={() => (editingPath = false)}
          />
        </form>
      {:else}
        <div
          class="flex h-8 min-w-0 flex-1 items-center gap-0.5 rounded-md border border-border bg-surface-input pl-1 pr-0.5"
        >
          <nav class="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto scrollbar-none" aria-label="Path">
            {#each pathBreadcrumbs as crumb, i (crumb.path)}
              {#if i > 0}
                <ChevronRight size={14} class="shrink-0 text-faint" />
              {/if}
              <button
                type="button"
                class="h-6 shrink-0 cursor-pointer whitespace-nowrap rounded-sm px-1.5 font-mono text-xs transition-colors hover:bg-surface-hover hover:text-primary {i === pathBreadcrumbs.length - 1 ? 'text-primary' : 'text-muted'}"
                onclick={() => loadDirectory(crumb.path)}
              >
                {crumb.label}
              </button>
            {/each}
          </nav>
          <button
            type="button"
            class="btn-icon btn-icon-sm"
            onclick={startEditingPath}
            aria-label="Edit path"
            title="Type a path"
          >
            <Pencil size={14} />
          </button>
        </div>
      {/if}

      <div class="segmented" role="group" aria-label="Layout">
        <button
          type="button"
          class="segmented-item {viewMode === 'list' ? 'segmented-item-active' : ''}"
          onclick={() => (viewMode = "list")}
          aria-pressed={viewMode === "list"}
          aria-label="List layout"
          title="List"
        >
          <List size={14} />
        </button>
        <button
          type="button"
          class="segmented-item {viewMode === 'grid' ? 'segmented-item-active' : ''}"
          onclick={() => (viewMode = "grid")}
          aria-pressed={viewMode === "grid"}
          aria-label="Grid layout"
          title="Grid"
        >
          <LayoutGrid size={14} />
        </button>
      </div>
    </div>

    <div class="view-toolbar">
      <label class="search-box w-56">
        <Search size={14} class="shrink-0" />
        <input
          type="text"
          placeholder="Filter files"
          bind:value={filterQuery}
          spellcheck="false"
          autocomplete="off"
        />
        {#if filterQuery}
          <button
            type="button"
            class="btn-icon btn-icon-sm -mr-1"
            onclick={() => (filterQuery = "")}
            aria-label="Clear filter"
          >
            <X size={14} />
          </button>
        {/if}
      </label>

      <div class="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto scrollbar-none" role="group" aria-label="Go to">
        {#each quickBookmarks as bm (bm.path)}
          <button
            type="button"
            class="chip {currentPath === bm.path ? 'chip-active' : ''}"
            onclick={() => loadDirectory(bm.path)}
            title={bm.path}
          >
            {bm.label}
          </button>
        {/each}
      </div>

      <button
        type="button"
        class="btn btn-ghost btn-sm"
        onclick={() => (showHidden = !showHidden)}
        aria-pressed={showHidden}
        title={showHidden ? "Hide dotfiles" : "Show dotfiles"}
      >
        {#if showHidden}<Eye size={14} />{:else}<EyeOff size={14} />{/if}
        Hidden files
        {#if !showHidden && hiddenCount > 0}<span class="count">{hiddenCount}</span>{/if}
      </button>
    </div>
  {/if}

  <div class="view-body">
    {#if errorMsg}
      <div class="alert alert-error mb-3" role="alert">
        <div class="flex min-w-0 flex-1 flex-col gap-0.5">
          <span class="alert-title">
            {isConnected ? "Couldn't read this directory" : `Couldn't connect to ${selectedConnection?.name ?? "host"}`}
          </span>
          <span class="break-all font-mono">{errorMsg}</span>
        </div>
        <button
          type="button"
          class="btn btn-secondary btn-sm shrink-0"
          onclick={() => loadDirectory(currentPath)}
          disabled={loading}
        >
          Retry
        </button>
      </div>
    {/if}

    {#if loading && (viewMode === "list" || !isConnected)}
      <div class="table-wrap" aria-busy="true" aria-label="Loading directory">
        <table class="data-table table-fixed">
          <thead>
            <tr>
              <th>Name</th>
              <th class="w-[96px] text-right">Size</th>
              <th class="hidden w-[148px] md:table-cell">Modified</th>
              <th class="hidden w-[120px] sm:table-cell">Permissions</th>
              <th class="w-[56px]"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each [62, 48, 70, 40, 55, 66, 44, 58] as w, i (i)}
              <tr>
                <td>
                  <div class="flex items-center gap-2.5">
                    <div class="skeleton size-3.5 shrink-0"></div>
                    <div class="skeleton h-3" style="width: {w}%"></div>
                  </div>
                </td>
                <td><div class="skeleton ml-auto h-3 w-10"></div></td>
                <td class="hidden md:table-cell"><div class="skeleton h-3 w-24"></div></td>
                <td class="hidden sm:table-cell"><div class="skeleton h-3 w-20"></div></td>
                <td></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if loading}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-2" aria-busy="true">
        {#each Array(9) as _, i (i)}
          <div class="card flex items-center gap-3 px-3 py-2.5">
            <div class="skeleton size-4 shrink-0"></div>
            <div class="flex flex-1 flex-col gap-1.5">
              <div class="skeleton h-3 w-3/4"></div>
              <div class="skeleton h-2.5 w-1/2"></div>
            </div>
          </div>
        {/each}
      </div>
    {:else if errorMsg}
      <!-- The alert above is the whole state; a stale listing would mislead. -->
    {:else if !isConnected}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><HardDrive size={18} /></div>
        {#if selectedConnection}
          <div class="empty-state-title">Browse remote files</div>
          <p class="empty-state-desc">
            Connect to {selectedConnection.name} to browse its files over SFTP.
          </p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={handleConnectClick}>
              Connect to {selectedConnection.name}
            </button>
          </div>
        {:else}
          <div class="empty-state-title">No hosts yet</div>
          <p class="empty-state-desc">Add a host first, then come back here to browse its files.</p>
        {/if}
      </div>
    {:else if filteredEntries.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Folder size={18} /></div>
        {#if filterQuery.trim()}
          <div class="empty-state-title">No files match</div>
          <p class="empty-state-desc">Nothing in {currentPath} matches “{filterQuery.trim()}”.</p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={() => (filterQuery = "")}>Clear filter</button>
          </div>
        {:else if hiddenCount > 0}
          <div class="empty-state-title">Only hidden files here</div>
          <p class="empty-state-desc">{currentPath} contains {hiddenCount} hidden {hiddenCount === 1 ? "entry" : "entries"}.</p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={() => (showHidden = true)}>Show hidden files</button>
          </div>
        {:else}
          <div class="empty-state-title">This folder is empty</div>
          <p class="empty-state-desc">{currentPath} has no files or folders.</p>
          {#if canGoUp}
            <div class="empty-state-action">
              <button type="button" class="btn btn-secondary" onclick={navigateUp}>
                <ArrowUp size={14} />
                Parent directory
              </button>
            </div>
          {/if}
        {/if}
      </div>
    {:else if viewMode === "list"}
      <div class="table-wrap">
        <table class="data-table table-fixed">
          <thead>
            <tr>
              <th>Name</th>
              <th class="w-[96px] text-right">Size</th>
              <th class="hidden w-[148px] md:table-cell">Modified</th>
              <th class="hidden w-[120px] sm:table-cell">Permissions</th>
              <th class="w-[56px]"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredEntries as entry (entry.path)}
              {@const Icon = getFileIcon(entry)}
              <tr class="cursor-default" ondblclick={() => entry.is_dir && loadDirectory(entry.path)}>
                <td>
                  <button
                    type="button"
                    class="flex w-full min-w-0 cursor-pointer items-center gap-2.5 text-left"
                    onclick={() => handleEntryClick(entry)}
                    title={entry.is_dir ? `Open ${entry.name}` : `Copy path of ${entry.name}`}
                  >
                    <Icon size={14} class="shrink-0 {entry.is_dir ? 'text-accent' : 'text-muted'}" />
                    <span class="truncate {entry.is_dir ? 'font-medium text-primary' : 'text-primary'}">{entry.name}</span>
                  </button>
                </td>
                <td class="text-right font-mono text-xs tabular-nums {entry.is_dir ? 'text-muted' : 'text-secondary'}">
                  {entry.is_dir ? "—" : formatBytes(entry.size)}
                </td>
                <td class="hidden truncate text-xs text-muted tabular-nums md:table-cell">{entry.modified || "—"}</td>
                <td class="hidden truncate font-mono text-xs text-muted sm:table-cell">{entry.permissions || "—"}</td>
                <td>
                  <div class="row-actions">{@render copyButton(entry)}</div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-2">
        {#each filteredEntries as entry (entry.path)}
          {@const Icon = getFileIcon(entry)}
          <div class="card card-interactive group flex min-w-0 items-center gap-1 py-2 pl-3 pr-1.5">
            <button
              type="button"
              class="flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 text-left"
              onclick={() => handleEntryClick(entry)}
              title={entry.is_dir ? `Open ${entry.name}` : `Copy path of ${entry.name}`}
            >
              <Icon size={16} class="shrink-0 {entry.is_dir ? 'text-accent' : 'text-muted'}" />
              <span class="flex min-w-0 flex-col">
                <span class="truncate text-sm text-primary {entry.is_dir ? 'font-medium' : ''}">{entry.name}</span>
                <span class="truncate font-mono text-2xs text-muted">
                  {entry.is_dir ? "Folder" : formatBytes(entry.size)} · {entry.permissions || "—"}
                </span>
              </span>
            </button>
            <div class="row-actions">{@render copyButton(entry)}</div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if isConnected}
    <footer class="flex h-8 shrink-0 items-center justify-between gap-4 border-t border-border-subtle px-6 text-xs text-muted">
      <span class="tabular-nums">
        {summary.dirs} {summary.dirs === 1 ? "folder" : "folders"} · {summary.files} {summary.files === 1 ? "file" : "files"} · {formatBytes(summary.totalSize)}
        {#if filterQuery.trim()}
          · {filteredEntries.length} shown
        {/if}
      </span>
      <span class="truncate font-mono" title={currentPath}>{currentPath}</span>
    </footer>
  {/if}
</div>
