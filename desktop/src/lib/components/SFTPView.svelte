<script lang="ts">
  import {
    ArrowLeft,
    Check,
    ChevronRight,
    Copy,
    File,
    FileCode,
    FileText,
    Folder,
    FolderGit2,
    FolderOpen,
    HardDrive,
    Home,
    Image,
    LayoutGrid,
    List,
    Lock,
    Power,
    RefreshCw,
    Search,
    Server,
    Shield,
    Sparkles,
    Terminal,
    Upload,
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

  let selectedConnection = $state<Connection | null>(connections.length > 0 ? connections[0] : null);
  let currentPath = $state<string>("/");
  let pathInput = $state<string>("/");
  let filterQuery = $state<string>("");
  let viewMode = $state<"list" | "grid">("list");
  let entries = $state<RemoteFileEntry[]>([]);
  let isConnected = $state<boolean>(false);
  let loading = $state<boolean>(false);
  let errorMsg = $state<string | null>(null);
  let copiedPath = $state<string | null>(null);

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
    void loadDirectory(pathInput || "/");
  }

  function handleDisconnect() {
    isConnected = false;
    entries = [];
    errorMsg = null;
  }

  function handleNavigate(path: string) {
    void loadDirectory(path);
  }

  function handleNavigateUp() {
    if (currentPath === "/" || currentPath === ".") return;
    const parts = currentPath.split("/").filter(Boolean);
    parts.pop();
    const parentPath = "/" + parts.join("/");
    handleNavigate(parentPath || "/");
  }

  function handleSelectConnection(name: string) {
    const conn = connections.find((c) => c.name === name);
    if (conn) {
      selectedConnection = conn;
      currentPath = "/";
      pathInput = "/";
      isConnected = false;
      entries = [];
      errorMsg = null;
    }
  }

  function copyPathToClipboard(path: string) {
    navigator.clipboard.writeText(path).then(() => {
      copiedPath = path;
      setTimeout(() => (copiedPath = null), 2000);
      notify(`Copied path: ${path}`, "info");
    });
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  function getFileTypeStyle(entry: RemoteFileEntry) {
    if (entry.is_dir) {
      return {
        icon: Folder,
        colorClass: "text-warning bg-amber-400/10 border-amber-400/20",
        badge: "Directory",
      };
    }
    const name = entry.name.toLowerCase();
    if (name.endsWith(".png") || name.endsWith(".jpg") || name.endsWith(".svg") || name.endsWith(".webp") || name.endsWith(".gif")) {
      return {
        icon: Image,
        colorClass: "text-purple-400 bg-purple-400/10 border-purple-400/20",
        badge: "Image",
      };
    }
    if (name.endsWith(".json") || name.endsWith(".js") || name.endsWith(".ts") || name.endsWith(".py") || name.endsWith(".rs") || name.endsWith(".html") || name.endsWith(".css") || name.endsWith(".sh")) {
      return {
        icon: FileCode,
        colorClass: "text-cyan-400 bg-cyan-400/10 border-cyan-400/20",
        badge: "Code",
      };
    }
    if (name.endsWith(".txt") || name.endsWith(".log") || name.endsWith(".md") || name.endsWith(".conf") || name.endsWith(".yaml") || name.endsWith(".yml")) {
      return {
        icon: FileText,
        colorClass: "text-running bg-running/10 border-running/20",
        badge: "Doc",
      };
    }
    if (name.endsWith(".zip") || name.endsWith(".tar") || name.endsWith(".gz") || name.endsWith(".7z") || name.endsWith(".rar")) {
      return {
        icon: HardDrive,
        colorClass: "text-error bg-rose-400/10 border-error/20",
        badge: "Archive",
      };
    }
    return {
      icon: File,
      colorClass: "text-secondary bg-surface-input border-border",
      badge: "File",
    };
  }
</script>

<div class="flex flex-col flex-1 min-h-0 w-full bg-surface p-5 gap-4">
  <!-- Top Header Bar -->
  <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 shrink-0 pb-1 border-b border-border/40">
    <div class="flex items-center gap-3">
      <div class="p-2.5 rounded-xl bg-accent/10 border border-accent/20 text-accent shadow-xs">
        <HardDrive size={22} />
      </div>
      <div>
        <h2 class="text-base font-bold text-primary m-0 flex items-center gap-2">
          <span>SFTP Graphical Explorer</span>
          {#if isConnected}
            <span class="px-2 py-0.5 rounded-full bg-success/10 border border-success/20 text-running text-[10px] font-semibold flex items-center gap-1">
              <span class="w-1.5 h-1.5 rounded-full bg-running animate-pulse"></span>
              Connected
            </span>
          {:else}
            <span class="px-2 py-0.5 rounded-full bg-surface-input border border-border text-muted text-[10px] font-semibold">
              Idle
            </span>
          {/if}
        </h2>
        <p class="text-xs text-muted mt-0.5 m-0">
          Browse remote filesystems, inspect POSIX permissions, and manage remote directory structure over SSH
        </p>
      </div>
    </div>

    <!-- Host Connection Selector -->
    <div class="w-72 shrink-0 flex items-center gap-2">
      <CustomSelect
        id="sftp-connection-select"
        options={connections.map((c) => ({ value: c.name, label: `${c.name} (${c.user}@${c.host})` }))}
        value={selectedConnection?.name ?? ""}
        onChange={handleSelectConnection}
      />
    </div>
  </div>

  {#if !isConnected && !loading && !errorMsg}
    <!-- Initial Idle Lander Card (No auto-connection!) -->
    <div class="flex-1 flex flex-col items-center justify-center p-8 bg-surface-card border border-border rounded-xl text-center max-w-xl mx-auto my-auto gap-5 shadow-sm">
      <div class="p-4 rounded-2xl bg-accent/10 border border-accent/20 text-accent">
        <FolderOpen size={40} />
      </div>

      <div class="flex flex-col gap-1 max-w-md">
        <h3 class="text-lg font-bold text-primary m-0">SFTP Remote Browser</h3>
        <p class="text-xs text-muted m-0">
          Select an SSH server host and click <strong class="text-primary">Connect & Browse</strong> to open remote filesystem.
        </p>
      </div>

      {#if selectedConnection}
        <div class="w-full bg-surface-terminal p-3.5 rounded-xl border border-border flex items-center justify-between font-mono text-xs text-primary">
          <div class="flex items-center gap-2 min-w-0">
            <Server size={15} class="text-accent shrink-0" />
            <span class="font-semibold truncate">{selectedConnection.name}</span>
          </div>
          <span class="text-muted text-[11px] font-mono truncate">
            {selectedConnection.user}@{selectedConnection.host}:{selectedConnection.port}
          </span>
        </div>
      {/if}

      <!-- Path Input & Connect Button -->
      <form
        class="w-full flex flex-col sm:flex-row items-center gap-2.5"
        onsubmit={(e) => {
          e.preventDefault();
          handleConnectClick();
        }}
      >
        <input
          type="text"
          bind:value={pathInput}
          placeholder="Starting remote directory path (/ or ~)..."
          class="flex-1 w-full bg-surface-input border border-border text-primary py-2.5 px-3.5 rounded-xl outline-none text-xs font-mono transition-all focus:border-border-focus"
        />
        <button
          type="submit"
          class="w-full sm:w-auto px-5 py-2.5 rounded-xl bg-accent text-white font-semibold text-xs cursor-pointer hover:opacity-90 transition-all shadow-sm flex items-center justify-center gap-2 whitespace-nowrap"
        >
          <FolderOpen size={16} />
          <span>Connect & Browse</span>
        </button>
      </form>
    </div>
  {:else}
    <!-- Explorer Body Layout -->
    <div class="flex-1 min-h-0 flex gap-4">
      <!-- Left Bookmark & Host Sidebar -->
      <div class="w-56 shrink-0 bg-surface-card border border-border rounded-xl p-3.5 flex flex-col gap-4 hidden lg:flex">
        <!-- Connection Card -->
        {#if selectedConnection}
          <div class="bg-surface-terminal p-3 rounded-lg border border-border flex flex-col gap-2">
            <div class="flex items-center justify-between">
              <span class="text-[11px] font-bold text-primary truncate">{selectedConnection.name}</span>
              <Server size={13} class="text-accent shrink-0" />
            </div>
            <span class="font-mono text-[10px] text-muted truncate">
              {selectedConnection.user}@{selectedConnection.host}:{selectedConnection.port}
            </span>
            <button
              type="button"
              class="mt-1 w-full py-1 rounded bg-surface-input border border-border text-muted hover:text-danger hover:border-danger/30 text-[10px] font-semibold cursor-pointer transition-colors flex items-center justify-center gap-1"
              onclick={handleDisconnect}
            >
              <Power size={11} />
              <span>Disconnect</span>
            </button>
          </div>
        {/if}

        <!-- Quick Bookmarks -->
        <div class="flex flex-col gap-1">
          <span class="text-[10px] font-bold uppercase tracking-wider text-muted px-1 mb-1">Bookmarks</span>
          {#each quickBookmarks as bm (bm.path)}
            <button
              type="button"
              class="flex items-center gap-2.5 w-full px-2.5 py-1.5 rounded-lg text-xs font-medium text-secondary hover:text-primary hover:bg-white/[0.04] transition-all cursor-pointer border border-transparent hover:border-border/60"
              onclick={() => handleNavigate(bm.path)}
            >
              <bm.icon size={14} class="text-accent/80" />
              <span>{bm.label}</span>
            </button>
          {/each}
        </div>
      </div>

      <!-- Main File Pane -->
      <div class="flex-1 min-w-0 flex flex-col gap-3">
        <!-- Interactive Action & Breadcrumb Bar -->
        <div class="bg-surface-card border border-border rounded-xl p-2.5 flex items-center justify-between gap-3 shrink-0">
          <!-- Left: Up button & Interactive Breadcrumb Pills -->
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <button
              type="button"
              class="p-1.5 rounded-lg bg-surface-input border border-border text-secondary hover:text-primary hover:border-border-hover cursor-pointer transition-colors disabled:opacity-40 shrink-0"
              disabled={currentPath === "/" || currentPath === "." || loading}
              onclick={handleNavigateUp}
              title="Go to parent directory"
            >
              <ArrowLeft size={15} />
            </button>

            <!-- Breadcrumb Trail -->
            <div class="flex items-center gap-1 overflow-x-auto scrollbar-none py-0.5">
              {#each pathBreadcrumbs as crumb, i (crumb.path)}
                {#if i > 0}
                  <ChevronRight size={12} class="text-muted shrink-0" />
                {/if}
                <button
                  type="button"
                  class="px-2 py-1 rounded-md text-xs font-mono font-medium transition-all cursor-pointer whitespace-nowrap
                    {i === pathBreadcrumbs.length - 1 ? 'bg-accent/15 text-accent border border-accent/30 font-semibold' : 'text-secondary hover:text-primary hover:bg-white/5'}"
                  onclick={() => handleNavigate(crumb.path)}
                >
                  {crumb.label}
                </button>
              {/each}
            </div>
          </div>

          <!-- Right: Search, Refresh & View Mode Toggle -->
          <div class="flex items-center gap-2 shrink-0">
            <div class="relative w-44 hidden md:block">
              <Search size={13} class="absolute left-2.5 top-2.5 text-muted" />
              <input
                type="text"
                placeholder="Search files..."
                bind:value={filterQuery}
                class="w-full bg-surface-input border border-border text-primary py-1.5 pl-8 pr-3 rounded-lg outline-none text-xs transition-all focus:border-border-focus"
              />
            </div>

            <!-- View Mode Toggle -->
            <div class="flex border border-border rounded-lg p-0.5 bg-surface-input">
              <button
                type="button"
                class="p-1 rounded-md cursor-pointer transition-all text-muted hover:text-primary {viewMode === 'list' ? 'bg-accent text-white shadow-xs' : ''}"
                onclick={() => (viewMode = "list")}
                title="List View"
              >
                <List size={14} />
              </button>
              <button
                type="button"
                class="p-1 rounded-md cursor-pointer transition-all text-muted hover:text-primary {viewMode === 'grid' ? 'bg-accent text-white shadow-xs' : ''}"
                onclick={() => (viewMode = "grid")}
                title="Grid View"
              >
                <LayoutGrid size={14} />
              </button>
            </div>

            <button
              type="button"
              class="p-1.5 rounded-lg bg-surface-input border border-border text-secondary hover:text-primary hover:border-border-hover cursor-pointer transition-colors"
              onclick={() => loadDirectory(currentPath)}
              title="Refresh directory"
            >
              <RefreshCw size={15} class={loading ? "animate-spin text-accent" : ""} />
            </button>
          </div>
        </div>

        <!-- File Content Explorer Container -->
        <div class="flex-1 min-h-0 bg-surface-terminal border border-border rounded-xl overflow-y-auto flex flex-col relative">
          {#if loading}
            <div class="flex-1 flex flex-col items-center justify-center p-8 text-center text-muted gap-3">
              <RefreshCw size={32} class="animate-spin text-accent" />
              <span class="text-xs font-medium">Fetching remote directory contents...</span>
            </div>
          {:else if errorMsg}
            <div class="flex-1 flex flex-col items-center justify-center p-8 text-center text-error gap-2">
              <Lock size={36} class="text-danger mb-1" />
              <h4 class="text-sm font-bold m-0 text-primary">Unable to Access Remote Path</h4>
              <p class="text-xs text-muted max-w-md m-0 font-mono bg-surface-input p-2.5 rounded border border-border leading-relaxed">{errorMsg}</p>
              <div class="flex items-center gap-2 mt-2">
                <button
                  type="button"
                  class="px-4 py-2 rounded-lg bg-surface-input border border-border text-primary text-xs font-semibold cursor-pointer hover:bg-white/5"
                  onclick={() => handleNavigate("/")}
                >
                  Try Root Path ( / )
                </button>
                <button
                  type="button"
                  class="px-4 py-2 rounded-lg bg-accent text-white text-xs font-semibold cursor-pointer hover:opacity-90 shadow-sm"
                  onclick={handleConnectClick}
                >
                  Retry Connection
                </button>
              </div>
            </div>
          {:else if filteredEntries.length === 0}
            <div class="flex-1 flex flex-col items-center justify-center p-8 text-center text-muted gap-2">
              <Folder size={36} class="text-muted/50 mb-1" />
              <span class="text-xs font-semibold text-secondary">No files or directories found</span>
            </div>
          {:else if viewMode === "list"}
            <!-- List View Table -->
            <div class="w-full text-xs text-left">
              <div class="grid grid-cols-12 gap-2 px-4 py-2.5 font-bold text-muted uppercase tracking-wider border-b border-border bg-surface-card/80 sticky top-0 backdrop-blur-md z-10 text-[10px]">
                <span class="col-span-6 sm:col-span-5">Name</span>
                <span class="col-span-2 hidden sm:block">Size</span>
                <span class="col-span-3 hidden md:block">Permissions</span>
                <span class="col-span-3 sm:col-span-2 md:col-span-2 text-right">Actions</span>
              </div>

              <div class="divide-y divide-border/40">
                {#each filteredEntries as entry (entry.path)}
                  {@const typeStyle = getFileTypeStyle(entry)}
                  {@const IconComponent = typeStyle.icon}
                  <div
                    class="grid grid-cols-12 gap-2 px-4 py-2.5 items-center hover:bg-white/[0.04] transition-colors group cursor-pointer"
                    onclick={() => entry.is_dir && handleNavigate(entry.path)}
                    role="button"
                    tabindex="0"
                    onkeydown={(e) => e.key === "Enter" && entry.is_dir && handleNavigate(entry.path)}
                  >
                    <!-- Name & Color Icon Badge -->
                    <div class="col-span-6 sm:col-span-5 flex items-center gap-2.5 min-w-0">
                      <div class="p-1 rounded border shrink-0 {typeStyle.colorClass}">
                        <IconComponent size={14} />
                      </div>
                      <span class="font-mono text-xs text-primary truncate font-medium group-hover:text-accent transition-colors">
                        {entry.name}
                      </span>
                    </div>

                    <!-- File Size -->
                    <div class="col-span-2 hidden sm:block font-mono text-muted text-[11px]">
                      {entry.is_dir ? "--" : formatBytes(entry.size)}
                    </div>

                    <!-- POSIX Permissions Badge -->
                    <div class="col-span-3 hidden md:block font-mono text-[11px]">
                      <span class="px-2 py-0.5 rounded bg-surface-input border border-border text-muted">
                        {entry.permissions}
                      </span>
                    </div>

                    <!-- Quick Row Actions -->
                    <div class="col-span-3 sm:col-span-2 md:col-span-2 flex items-center justify-end gap-1" onclick={(e) => e.stopPropagation()} role="none">
                      <button
                        type="button"
                        class="p-1.5 rounded text-muted hover:text-primary hover:bg-white/10 transition-colors"
                        title="Copy remote path"
                        onclick={() => copyPathToClipboard(entry.path)}
                      >
                        {#if copiedPath === entry.path}
                          <Check size={14} class="text-running" />
                        {:else}
                          <Copy size={14} />
                        {/if}
                      </button>
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          {:else}
            <!-- Grid View Cards -->
            <div class="p-4 grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
              {#each filteredEntries as entry (entry.path)}
                {@const typeStyle = getFileTypeStyle(entry)}
                {@const IconComponent = typeStyle.icon}
                <div
                  class="bg-surface-card border border-border rounded-xl p-3 flex flex-col justify-between gap-3 hover:border-border-hover hover:shadow-md transition-all group cursor-pointer relative"
                  onclick={() => entry.is_dir && handleNavigate(entry.path)}
                  role="button"
                  tabindex="0"
                  onkeydown={(e) => e.key === "Enter" && entry.is_dir && handleNavigate(entry.path)}
                >
                  <div class="flex items-start justify-between gap-2">
                    <div class="p-2 rounded-lg border {typeStyle.colorClass}">
                      <IconComponent size={20} />
                    </div>
                    <span class="px-1.5 py-0.5 rounded bg-surface-input border border-border text-[9px] font-semibold text-muted">
                      {typeStyle.badge}
                    </span>
                  </div>

                  <div class="flex flex-col min-w-0">
                    <span class="font-mono text-xs font-semibold text-primary truncate group-hover:text-accent transition-colors" title={entry.name}>
                      {entry.name}
                    </span>
                    <span class="font-mono text-[10px] text-muted mt-0.5">
                      {entry.is_dir ? "Directory" : formatBytes(entry.size)}
                    </span>
                  </div>

                  <div class="flex items-center justify-between pt-2 border-t border-border/40 text-[10px] text-muted font-mono" onclick={(e) => e.stopPropagation()} role="none">
                    <span>{entry.permissions}</span>
                    <button
                      type="button"
                      class="p-1 rounded text-muted hover:text-primary transition-colors"
                      title="Copy path"
                      onclick={() => copyPathToClipboard(entry.path)}
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
          {/if}
        </div>

        <!-- Bottom Directory Summary Footer Bar -->
        <div class="bg-surface-card border border-border rounded-xl px-4 py-2 flex items-center justify-between text-xs text-muted shrink-0">
          <div class="flex items-center gap-3">
            <span class="font-medium text-primary">{entries.length} items total</span>
            <span>•</span>
            <span>{summary.dirs} folders</span>
            <span>•</span>
            <span>{summary.files} files</span>
          </div>
          <span class="font-mono text-[11px] font-semibold text-accent">Total: {formatBytes(summary.totalSize)}</span>
        </div>
      </div>
    </div>
  {/if}
</div>
