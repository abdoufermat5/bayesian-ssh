<script lang="ts">
  import {
    ArrowLeft,
    Check,
    ChevronRight,
    Copy,
    File,
    FileArchive,
    FileCode,
    FileText,
    Folder,
    FolderGit2,
    HardDrive,
    Home,
    Image,
    LayoutGrid,
    List,
    Power,
    RefreshCw,
    Search,
    Shield,
    Terminal,
    X,
  } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Connection } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { notify } from "$lib/stores/notifications.svelte";

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
  let currentPath = $state<string>("/");
  let pathInput = $state<string>("/");
  let filterQuery = $state<string>("");
  let viewMode = $state<"list" | "grid">("list");
  let entries = $state<RemoteFileEntry[]>([]);
  let isConnected = $state<boolean>(false);
  let loading = $state<boolean>(false);
  let errorMsg = $state<string | null>(null);
  let copiedPath = $state<string | null>(null);

  const selectedConnection = $derived.by(() => {
    if (selectedConnectionId) {
      return connections.find((c) => c.id === selectedConnectionId) ?? connections[0] ?? null;
    }
    return connections.length > 0 ? connections[0] : null;
  });

  const quickBookmarks = [
    { label: "Root", path: "/", icon: HardDrive },
    { label: "Home", path: "~", icon: Home },
    { label: "Web", path: "/var/www", icon: FolderGit2 },
    { label: "Config", path: "/etc", icon: Shield },
    { label: "Logs", path: "/var/log", icon: FileText },
    { label: "Temp", path: "/tmp", icon: Terminal },
  ];

  const pathBreadcrumbs = $derived.by(() => {
    if (currentPath === "/" || currentPath === ".") {
      return [{ label: "root", path: "/" }];
    }
    const parts = currentPath.split("/").filter(Boolean);
    const crumbs = [{ label: "root", path: "/" }];
    let acc = "";
    for (const part of parts) {
      acc += "/" + part;
      crumbs.push({ label: part, path: acc });
    }
    return crumbs;
  });

  const filteredEntries = $derived.by(() => {
    if (!filterQuery.trim()) return entries;
    const query = filterQuery.toLowerCase();
    return entries.filter(
      (entry) =>
        entry.name.toLowerCase().includes(query) ||
        entry.permissions.toLowerCase().includes(query),
    );
  });

  const summary = $derived.by(() => {
    const dirs = entries.filter((e) => e.is_dir).length;
    const files = entries.filter((e) => !e.is_dir).length;
    const totalSize = entries.reduce((acc, e) => acc + (e.is_dir ? 0 : e.size), 0);
    return { dirs, files, totalSize };
  });

  async function loadDirectory(path: string) {
    if (!selectedConnection) return;
    loading = true;
    errorMsg = null;
    try {
      const res = await invoke<RemoteFileEntry[]>("list_remote_directory", {
        connectionName: selectedConnection.name,
        remotePath: path,
      });
      entries = res;
      currentPath = path;
      pathInput = path;
      isConnected = true;
    } catch (err: unknown) {
      errorMsg = String(err);
      notify(`SFTP error: ${err}`, "error");
    } finally {
      loading = false;
    }
  }

  function handleConnectClick() {
    if (isConnected) {
      isConnected = false;
      entries = [];
      errorMsg = null;
      notify("Disconnected from SFTP session", "info");
    } else {
      loadDirectory(currentPath);
    }
  }

  function goToPath() {
    const nextPath = pathInput.trim() || "/";
    loadDirectory(nextPath);
  }

  function navigateUp() {
    if (currentPath === "/" || currentPath === ".") return;
    const parts = currentPath.split("/").filter(Boolean);
    parts.pop();
    const parent = parts.length === 0 ? "/" : "/" + parts.join("/");
    loadDirectory(parent);
  }

  function handleEntryClick(entry: RemoteFileEntry) {
    if (entry.is_dir) {
      loadDirectory(entry.path);
    } else {
      copyPath(entry.path);
    }
  }

  function copyPath(p: string) {
    navigator.clipboard.writeText(p);
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
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  function getFileIcon(entry: RemoteFileEntry) {
    if (entry.is_dir) return Folder;
    const ext = entry.name.split(".").pop()?.toLowerCase();
    if (["js", "ts", "py", "rs", "go", "json", "yaml", "yml", "toml", "html", "css"].includes(ext ?? "")) {
      return FileCode;
    }
    if (["zip", "tar", "gz", "bz2", "xz", "7z"].includes(ext ?? "")) {
      return FileArchive;
    }
    if (["png", "jpg", "jpeg", "svg", "webp", "gif"].includes(ext ?? "")) {
      return Image;
    }
    return File;
  }
</script>

<div class="flex min-h-0 w-full flex-1 flex-col overflow-hidden bg-surface text-[13px] text-primary select-none">
  <div class="view-header">
    <div class="flex min-w-0 items-center gap-3">
      <div class="icon-tile rounded-md">
        <HardDrive size={16} />
      </div>
      <div class="min-w-0">
        <h2 class="m-0 flex items-center gap-2 truncate text-sm font-bold tracking-tight text-primary">
          SFTP
          {#if isConnected}
            <span class="badge badge-running">Connected</span>
          {:else}
            <span class="badge badge-subtle">Offline</span>
          {/if}
        </h2>
        <p class="m-0 truncate text-xs text-muted">Remote file browser and path operations</p>
      </div>
    </div>

    <div class="flex min-w-0 flex-1 items-center justify-end gap-2 sm:flex-none">
      <div class="w-full sm:w-64">
        <CustomSelect
          options={connections.map((c) => ({ value: c.id, label: `${c.name} (${c.user}@${c.host})` }))}
          value={selectedConnection?.id ?? ""}
          onChange={(val) => {
            selectedConnectionId = val;
            isConnected = false;
            entries = [];
          }}
        />
      </div>

      <button
        type="button"
        class="btn {isConnected ? 'btn-danger' : 'btn-primary'}"
        onclick={handleConnectClick}
        disabled={!selectedConnection || loading}
      >
        {#if loading}
          <RefreshCw size={13} class="animate-spin" />
          <span>Connecting</span>
        {:else}
          <Power size={13} />
          <span>{isConnected ? "Disconnect" : "Connect"}</span>
        {/if}
      </button>
    </div>
  </div>

  {#if isConnected}
    <div class="toolbar-band">
      <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
        <button
          type="button"
          class="btn btn-secondary btn-sm"
          onclick={navigateUp}
          disabled={currentPath === "/" || currentPath === "." || loading}
          title="Go to parent directory"
        >
          <ArrowLeft size={13} />
          <span>Up</span>
        </button>

        <form
          class="flex min-w-0 flex-1 items-center gap-1 rounded-md border border-border bg-surface-input px-2 py-1.5"
          onsubmit={(event) => {
            event.preventDefault();
            goToPath();
          }}
        >
          <div class="flex min-w-0 items-center gap-1 overflow-x-auto">
            {#each pathBreadcrumbs as crumb, i}
              {#if i > 0}
                <ChevronRight size={11} class="shrink-0 text-muted" />
              {/if}
              <button
                type="button"
                class="border-none bg-transparent font-mono text-xs text-secondary transition-colors hover:text-primary whitespace-nowrap"
                onclick={() => loadDirectory(crumb.path)}
              >
                {crumb.label}
              </button>
            {/each}
          </div>
          <input
            type="text"
            bind:value={pathInput}
            class="ml-auto min-w-[80px] flex-1 border-none bg-transparent text-right font-mono text-xs text-primary outline-none"
            aria-label="Remote path"
          />
        </form>
      </div>

      <div class="flex items-center gap-1.5">
        <button
          type="button"
          class="btn-icon {viewMode === 'list' ? 'bg-surface-hover text-primary' : ''}"
          onclick={() => (viewMode = "list")}
          title="List view"
        >
          <List size={14} />
        </button>
        <button
          type="button"
          class="btn-icon {viewMode === 'grid' ? 'bg-surface-hover text-primary' : ''}"
          onclick={() => (viewMode = "grid")}
          title="Grid view"
        >
          <LayoutGrid size={14} />
        </button>
        <button
          type="button"
          class="btn-icon"
          onclick={() => loadDirectory(currentPath)}
          disabled={loading}
          title="Refresh current folder"
        >
          <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
        </button>
      </div>
    </div>

    <div class="toolbar-band border-t-0">
      <div class="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
        {#each quickBookmarks as bm}
          <button
            type="button"
            class="btn btn-secondary btn-sm"
            onclick={() => loadDirectory(bm.path)}
          >
            <bm.icon size={12} />
            <span>{bm.label}</span>
          </button>
        {/each}
      </div>

      <div class="search-box w-full sm:w-56">
        <Search size={13} class="shrink-0 text-muted" />
        <input
          type="text"
          placeholder="Filter files..."
          bind:value={filterQuery}
          class="w-full border-none bg-transparent text-xs text-primary outline-none placeholder:text-muted"
        />
        {#if filterQuery}
          <button type="button" class="btn-icon p-0.5" onclick={() => (filterQuery = "")} title="Clear filter">
            <X size={11} />
          </button>
        {/if}
      </div>
    </div>

    <div class="view-content">
      {#if loading}
        <div class="empty-state">
          <RefreshCw size={24} class="animate-spin text-accent" />
          <span class="text-xs text-muted">Reading remote directory...</span>
        </div>
      {:else if errorMsg}
        <div class="alert alert-error">
          <div>
            <p class="alert-title m-0">Failed to read directory</p>
            <p class="m-0 font-mono text-xs">{errorMsg}</p>
          </div>
        </div>
      {:else if filteredEntries.length > 0}
        {#if viewMode === "list"}
          <div class="data-table overflow-x-auto">
            <table class="w-full border-collapse">
              <thead>
                <tr class="border-b border-border bg-surface-input/80 text-left table-header">
                  <th class="px-4 py-2.5 font-bold">Name</th>
                  <th class="px-4 py-2.5 font-bold">Size</th>
                  <th class="hidden sm:table-cell px-4 py-2.5 font-bold">Permissions</th>
                  <th class="hidden md:table-cell px-4 py-2.5 font-bold">Modified</th>
                  <th class="px-4 py-2.5 text-right font-bold">Actions</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-border/60">
                {#each filteredEntries as entry}
                  {@const Icon = getFileIcon(entry)}
                  <tr class="row border-0 text-xs text-secondary">
                    <td class="max-w-[240px] sm:max-w-[320px] px-4 py-2.5">
                      <button
                        type="button"
                        class="flex min-w-0 items-center gap-2.5 border-none bg-transparent text-left text-xs text-secondary"
                        onclick={() => handleEntryClick(entry)}
                        title={entry.is_dir ? "Open directory" : "Copy full remote path"}
                      >
                        <Icon size={15} class={entry.is_dir ? "shrink-0 text-accent" : "shrink-0 text-muted"} />
                        <span class="truncate {entry.is_dir ? 'font-semibold text-primary' : ''}">{entry.name}</span>
                      </button>
                    </td>
                    <td class="px-4 py-2.5 font-mono text-xs text-muted">{entry.is_dir ? "-" : formatBytes(entry.size)}</td>
                    <td class="hidden sm:table-cell px-4 py-2.5">
                      <span class="tag">{entry.permissions || "rw-r--r--"}</span>
                    </td>
                    <td class="hidden md:table-cell px-4 py-2.5 font-mono text-xs text-muted">{entry.modified || "-"}</td>
                    <td class="px-4 py-2.5 text-right">
                      <button
                        type="button"
                        class="btn-icon"
                        onclick={() => copyPath(entry.path)}
                        title="Copy full remote path"
                      >
                        {#if copiedPath === entry.path}
                          <Check size={12} class="text-running" />
                        {:else}
                          <Copy size={12} />
                        {/if}
                      </button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <div class="grid grid-cols-1 gap-2 md:grid-cols-2 xl:grid-cols-3">
            {#each filteredEntries as entry}
              {@const Icon = getFileIcon(entry)}
              <section class="panel flex min-w-0 items-center gap-3 px-3 py-2.5">
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-2 border-none bg-transparent text-left"
                  onclick={() => handleEntryClick(entry)}
                  title={entry.is_dir ? "Open directory" : "Copy full remote path"}
                >
                  <Icon size={16} class={entry.is_dir ? "shrink-0 text-accent" : "shrink-0 text-muted"} />
                  <div class="min-w-0">
                    <p class="m-0 truncate text-xs font-semibold text-primary">{entry.name}</p>
                    <p class="m-0 font-mono text-xs text-muted">{entry.is_dir ? "Folder" : formatBytes(entry.size)} · {entry.permissions || "rw-r--r--"}</p>
                  </div>
                </button>
                <button type="button" class="btn-icon" onclick={() => copyPath(entry.path)} title="Copy full remote path">
                  {#if copiedPath === entry.path}
                    <Check size={12} class="text-running" />
                  {:else}
                    <Copy size={12} />
                  {/if}
                </button>
              </section>
            {/each}
          </div>
        {/if}
      {:else}
        <div class="empty-state empty-state-dashed">
          <div class="empty-state-icon">
            <Folder size={22} />
          </div>
          <p class="empty-state-title">Empty directory</p>
          <p class="empty-state-desc">No files or folders found in {currentPath}</p>
        </div>
      {/if}
    </div>

    <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-border bg-surface-input/40 px-6 py-2 text-xs text-muted">
      <div class="flex items-center gap-4">
        <span>{summary.dirs} folders</span>
        <span>{summary.files} files</span>
        <span>Total {formatBytes(summary.totalSize)}</span>
      </div>
      <div class="max-w-full truncate font-mono">{currentPath}</div>
    </div>
  {:else}
    <div class="view-content flex items-center justify-center">
      <div class="empty-state empty-state-dashed">
        <div class="empty-state-icon">
          <HardDrive size={22} />
        </div>
        <h3 class="empty-state-title">No SFTP session</h3>
        <p class="empty-state-desc">Select an SSH host and connect to browse remote paths.</p>
        {#if selectedConnection}
          <button type="button" class="btn btn-primary empty-state-action" onclick={handleConnectClick}>
            <Power size={14} />
            <span>Connect to {selectedConnection.name}</span>
          </button>
        {/if}
      </div>
    </div>
  {/if}
</div>
