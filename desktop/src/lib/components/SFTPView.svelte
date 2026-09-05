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
    Server,
    Shield,
    Terminal,
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

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden bg-surface select-none">
  <!-- SFTP Header Control Bar -->
  <div class="px-6 py-4 border-b border-border flex items-center justify-between gap-4 shrink-0 bg-surface-input/30 flex-wrap">
    <div class="flex items-center gap-3 min-w-0">
      <div class="w-8 h-8 rounded-xl bg-accent/15 border border-accent/30 flex items-center justify-center text-accent shrink-0">
        <HardDrive size={18} />
      </div>
      <div>
        <h2 class="text-sm font-bold text-primary tracking-tight m-0 flex items-center gap-2">
          SFTP File Explorer
          {#if isConnected}
            <span class="badge-pill bg-running/15 text-running border border-running/30 text-[10px]">
              Connected
            </span>
          {/if}
        </h2>
        <p class="text-[11px] text-muted m-0">Remote file browser and path manager over SSH</p>
      </div>
    </div>

    <!-- Target Server Picker & Connect Toggle -->
    <div class="flex items-center gap-2">
      <div class="w-56">
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
        class="btn {isConnected ? 'btn-danger' : 'btn-primary'} shadow-sm"
        onclick={handleConnectClick}
        disabled={!selectedConnection || loading}
      >
        {#if loading}
          <RefreshCw size={13} class="animate-spin" />
          <span>Connecting...</span>
        {:else if isConnected}
          <Power size={13} />
          <span>Disconnect</span>
        {:else}
          <Power size={13} />
          <span>Connect SFTP</span>
        {/if}
      </button>

      {#if isConnected}
        <button
          type="button"
          class="btn-icon p-1.5 border border-border rounded-lg bg-surface hover:text-primary"
          onclick={() => loadDirectory(currentPath)}
          disabled={loading}
          title="Refresh current folder"
        >
          <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
        </button>
      {/if}
    </div>
  </div>

  {#if isConnected}
    <!-- Path Breadcrumb & Bookmarks Bar -->
    <div class="px-6 py-2 border-b border-border/80 bg-surface-input/10 flex items-center justify-between gap-3 shrink-0 flex-wrap">
      <!-- Breadcrumb Bar -->
      <div class="flex items-center gap-1.5 flex-1 min-w-[280px]">
        <button
          type="button"
          class="p-1 rounded-md text-muted hover:text-primary hover:bg-surface-hover border border-border cursor-pointer transition-colors bg-surface"
          onclick={navigateUp}
          disabled={currentPath === "/" || currentPath === "."}
          title="Go to parent directory"
        >
          <ArrowLeft size={13} />
        </button>

        <!-- Breadcrumbs -->
        <div class="flex items-center gap-1 text-xs font-mono bg-surface-input border border-border rounded-lg px-2.5 py-1 max-w-lg overflow-x-auto scrollbar-none">
          {#each pathBreadcrumbs as crumb, i}
            {#if i > 0}
              <ChevronRight size={11} class="text-muted shrink-0" />
            {/if}
            <button
              type="button"
              class="text-secondary hover:text-primary transition-colors cursor-pointer border-none bg-transparent whitespace-nowrap"
              onclick={() => loadDirectory(crumb.path)}
            >
              {crumb.label}
            </button>
          {/each}
        </div>
      </div>

      <!-- Quick Bookmarks -->
      <div class="flex items-center gap-1 overflow-x-auto scrollbar-none">
        {#each quickBookmarks as bm}
          <button
            type="button"
            class="flex items-center gap-1 px-2 py-0.5 rounded-md text-[10px] font-semibold text-muted hover:text-primary hover:bg-surface-hover border border-border/70 transition-colors cursor-pointer bg-surface"
            onclick={() => loadDirectory(bm.path)}
          >
            <bm.icon size={11} />
            <span>{bm.label}</span>
          </button>
        {/each}
      </div>

      <!-- Search Filter in Directory -->
      <div class="relative flex items-center bg-surface-input border border-border rounded-lg px-2 py-1 w-48">
        <Search size={12} class="text-muted mr-1.5 shrink-0" />
        <input
          type="text"
          placeholder="Filter files..."
          bind:value={filterQuery}
          class="bg-transparent border-none text-xs text-primary outline-none w-full placeholder:text-muted"
        />
      </div>
    </div>

    <!-- File Browser Table Area -->
    <div class="flex-1 min-h-0 overflow-y-auto px-6 py-3 scrollbar-none">
      {#if loading}
        <div class="py-20 flex flex-col items-center justify-center text-muted gap-2">
          <RefreshCw size={24} class="text-accent animate-spin" />
          <span class="text-xs">Reading remote directory...</span>
        </div>
      {:else if errorMsg}
        <div class="p-4 rounded-xl border border-error/30 bg-error/10 text-error text-xs">
          <p class="font-bold mb-1">Failed to read directory</p>
          <p class="font-mono">{errorMsg}</p>
        </div>
      {:else if filteredEntries.length > 0}
        <div class="border border-border rounded-xl overflow-hidden bg-surface-input/30 shadow-sm">
          <!-- Table Header -->
          <div class="flex items-center px-4 py-2 bg-surface-input/80 border-b border-border text-[10px] font-bold text-muted uppercase tracking-wider">
            <div class="flex-[4]">Name</div>
            <div class="flex-[1.5]">Size</div>
            <div class="flex-[1.5]">Permissions</div>
            <div class="flex-[2] hidden sm:block">Modified</div>
            <div class="w-16 text-right">Action</div>
          </div>

          <!-- Table Rows -->
          <div class="divide-y divide-border/60">
            {#each filteredEntries as entry}
              {@const Icon = getFileIcon(entry)}
              <div
                class="flex items-center px-4 py-2 text-xs text-secondary hover:bg-white/[0.04] hover:text-primary transition-colors cursor-pointer group"
                onclick={() => handleEntryClick(entry)}
                role="row"
                tabindex="0"
                onkeydown={(e) => e.key === "Enter" && handleEntryClick(entry)}
              >
                <!-- File Name & Icon -->
                <div class="flex-[4] flex items-center gap-2.5 min-w-0">
                  <Icon
                    size={15}
                    class={entry.is_dir ? "text-accent shrink-0" : "text-muted shrink-0"}
                  />
                  <span class="truncate {entry.is_dir ? 'font-semibold text-primary' : 'font-normal'}">
                    {entry.name}
                  </span>
                </div>

                <!-- Size -->
                <div class="flex-[1.5] font-mono text-[11px] text-muted">
                  {entry.is_dir ? "—" : formatBytes(entry.size)}
                </div>

                <!-- Permissions -->
                <div class="flex-[1.5] font-mono text-[10px] text-muted">
                  <span class="px-1.5 py-0.5 rounded bg-surface border border-border/70">
                    {entry.permissions || "rw-r--r--"}
                  </span>
                </div>

                <!-- Modified Date -->
                <div class="flex-[2] hidden sm:block font-mono text-[11px] text-muted truncate">
                  {entry.modified || "—"}
                </div>

                <!-- Copy Path Action -->
                <div class="w-16 flex justify-end">
                  <button
                    type="button"
                    class="p-1 rounded text-muted hover:text-primary hover:bg-surface-hover transition-colors border-none bg-transparent cursor-pointer"
                    onclick={(e) => {
                      e.stopPropagation();
                      copyPath(entry.path);
                    }}
                    title="Copy full remote path"
                  >
                    {#if copiedPath === entry.path}
                      <Check size={12} class="text-running" />
                    {:else}
                      <Copy size={12} />
                    {/if}
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <div class="py-16 flex flex-col items-center justify-center text-muted gap-2 border border-dashed border-border rounded-xl">
          <Folder size={32} class="opacity-30" />
          <p class="text-xs font-semibold text-primary">Empty Directory</p>
          <p class="text-[11px]">No files or folders found in {currentPath}</p>
        </div>
      {/if}
    </div>

    <!-- Summary Footer -->
    <div class="px-6 py-2 border-t border-border bg-surface-input/40 flex items-center justify-between text-[11px] text-muted shrink-0">
      <div class="flex items-center gap-4">
        <span>{summary.dirs} folders</span>
        <span>{summary.files} files</span>
        <span>Total: {formatBytes(summary.totalSize)}</span>
      </div>
      <div class="font-mono text-[10px]">
        {currentPath}
      </div>
    </div>
  {:else}
    <!-- Not Connected Splash -->
    <div class="flex-1 flex flex-col items-center justify-center p-12 text-center select-none">
      <div class="w-16 h-16 rounded-2xl bg-accent/10 border border-accent/25 flex items-center justify-center text-accent mb-4 shadow-sm">
        <HardDrive size={32} />
      </div>
      <h3 class="text-base font-bold text-primary mb-1.5">Remote File Browser</h3>
      <p class="text-xs text-muted max-w-sm leading-relaxed mb-6">
        Select an SSH host and connect to explore directories, inspect files, and copy remote paths directly within the application.
      </p>
      {#if selectedConnection}
        <button
          type="button"
          class="btn btn-primary px-4 py-2"
          onclick={handleConnectClick}
        >
          <Power size={14} />
          <span>Connect to {selectedConnection.name}</span>
        </button>
      {/if}
    </div>
  {/if}
</div>
