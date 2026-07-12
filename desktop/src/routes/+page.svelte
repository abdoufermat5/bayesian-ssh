<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";

  const appWindow = getCurrentWindow();

  async function handleWindowMinimize() {
    try {
      await appWindow.minimize();
    } catch (e) {
      console.error(e);
    }
  }

  async function handleWindowMaximize() {
    try {
      if (await appWindow.isMaximized()) {
        await appWindow.unmaximize();
      } else {
        await appWindow.maximize();
      }
    } catch (e) {
      console.error(e);
    }
  }

  async function handleWindowClose() {
    try {
      await appWindow.close();
    } catch (e) {
      console.error(e);
    }
  }
  
  import { 
    Plus, Trash2, Edit2, Play, Search, Server, Clock, Activity, 
    FolderPlus, Key, X, Shield, ChevronLeft, ChevronRight, LayoutGrid, List,
    CheckCircle2, AlertCircle, Copy, Check, TerminalSquare, RefreshCw, Settings
  } from "lucide-svelte";

  interface Connection {
    id: string;
    name: string;
    host: string;
    user: string;
    port: number;
    bastion?: string;
    bastion_user?: string;
    use_kerberos: boolean;
    key_path?: string;
    created_at: string;
    last_used?: string;
    tags: string[];
  }

  interface EnvInfo {
    name: string;
    is_active: boolean;
  }

  interface SessionHistoryEntry {
    id: string;
    name: string;
    started_at: string;
    ended_at?: string;
    status: string;
    exit_code?: number;
  }

  interface ConnectionStats {
    total_connections: number;
    most_used?: Connection;
    recently_used: Connection[];
    by_tag: Record<string, number>;
  }

  interface TerminalTab {
    id: string;
    name: string;
    connectionName: string;
    term?: Terminal;
    fitAddon?: FitAddon;
  }

  // App States
  let activeTab = $state("connections"); // "connections" | "history" | "terminals"
  let environments = $state<EnvInfo[]>([]);
  let activeEnv = $state("default");
  let connections = $state<Connection[]>([]);
  let searchQuery = $state("");
  let selectedTag = $state<string | null>(null);
  let stats = $state<ConnectionStats | null>(null);
  let history = $state<SessionHistoryEntry[]>([]);
  
  // Custom design states
  let viewMode = $state<"list" | "grid">("list");
  let sidebarCollapsed = $state(false);
  let selectedHostIndex = $state(0);

  // Terminal states
  let terminalTabs = $state<TerminalTab[]>([]);
  let activeTerminalTabId = $state<string | null>(null);

  // Edit/Add Modal States
  let showModal = $state(false);
  let isEditing = $state(false);
  let modalConnectionId = $state("");
  let modalName = $state("");
  let modalHost = $state("");
  let modalUser = $state("");
  let modalPort = $state(22);
  let modalUseKerberos = $state(false);
  let modalBastion = $state("");
  let modalBastionUser = $state("");
  let modalKeyPath = $state("");
  let modalTagsString = $state("");

  // Environment Modal States
  let showEnvModal = $state(false);
  let newEnvName = $state("");

  // Notification States
  let notificationText = $state("");
  let notificationType = $state<"success" | "error" | "info">("info");
  let showNotification = $state(false);
  let copiedId = $state<string | null>(null);

  // SSH Agent states
  let agentActive = $state(false);
  let agentSocket = $state<string | null>(null);
  let agentKeys = $state<string[]>([]);
  let showAgentModal = $state(false);
  let agentFeedback = $state<string | null>(null);
  let agentFeedbackType = $state<"success" | "error" | null>(null);

  // Settings states
  interface DesktopSettings {
    theme: string;
    auto_start_agent: boolean;
    custom_agent_socket: string;
    default_user: string;
    default_port: number;
    fuzzy_search: boolean;
  }
  let settings = $state<DesktopSettings>({
    theme: "zinc",
    auto_start_agent: false,
    custom_agent_socket: "",
    default_user: "root",
    default_port: 22,
    fuzzy_search: false
  });


  function notify(text: string, type: "success" | "error" | "info" = "info") {
    notificationText = text;
    notificationType = type;
    showNotification = true;
    setTimeout(() => {
      showNotification = false;
    }, 3000);
  }

  async function copyToClipboard(text: string, id: string) {
    try {
      await navigator.clipboard.writeText(text);
      copiedId = id;
      setTimeout(() => {
        if (copiedId === id) copiedId = null;
      }, 1500);
      notify("SSH command copied to clipboard", "success");
    } catch (e: any) {
      notify("Failed to copy", "error");
    }
  }

  // Load Initial Data
  async function loadData() {
    try {
      activeEnv = await invoke("get_active_env");
      environments = await invoke("list_environments");
      await loadConnections();
      await loadStats();
      await loadHistory();
      await loadSettings();
      await loadAgentStatus();
    } catch (e: any) {
      notify(e.toString(), "error");
    }
  }

  async function loadSettings() {
    try {
      const loaded: any = await invoke("load_desktop_settings");
      settings = {
        theme: loaded.theme || "zinc",
        auto_start_agent: loaded.auto_start_agent || false,
        custom_agent_socket: loaded.custom_agent_socket || "",
        default_user: loaded.default_user || "root",
        default_port: loaded.default_port || 22,
        fuzzy_search: loaded.fuzzy_search || false
      };
      applyTheme(settings.theme);
      
      // Auto-start agent if configured and not yet active
      if (settings.auto_start_agent && !agentActive) {
        await triggerStartAgent();
      }
    } catch (e: any) {
      console.error("Failed to load settings", e);
    }
  }

  async function saveSettings() {
    try {
      await invoke("save_desktop_settings", { settings });
      applyTheme(settings.theme);
      notify("Settings saved successfully", "success");
    } catch (e: any) {
      notify(`Failed to save settings: ${e}`, "error");
    }
  }

  function applyTheme(themeName: string) {
    document.documentElement.classList.remove("theme-zinc", "theme-cyberpunk", "theme-oled", "theme-slate");
    document.documentElement.classList.add(`theme-${themeName}`);
  }

  async function loadAgentStatus() {
    try {
      const status: any = await invoke("get_agent_status");
      agentActive = status.active;
      agentSocket = status.socket_path;
      agentKeys = status.keys;
    } catch (e) {
      console.error("Failed to load agent status", e);
    }
  }

  async function triggerStartAgent() {
    try {
      const status: any = await invoke("start_agent");
      agentActive = status.active;
      agentSocket = status.socket_path;
      agentKeys = status.keys;
      notify("SSH Agent started successfully", "success");
    } catch (e: any) {
      notify(`Failed to start agent: ${e}`, "error");
    }
  }

  async function triggerAddKey(keyPath: string) {
    try {
      const result: string = await invoke("add_key_to_agent", { keyPath });
      await loadAgentStatus();
      notify("Key added to SSH Agent successfully", "success");
    } catch (e: any) {
      notify(`Failed to add key: ${e}`, "error");
    }
  }

  async function selectAndAddKey() {
    try {
      const file = await invoke<string | null>("pick_key_file");
      if (file) {
        await triggerAddKey(file);
      }
    } catch (e) {
      console.error("Failed to pick key file", e);
    }
  }

  async function loadConnections() {
    try {
      connections = await invoke("get_connections", { 
        query: searchQuery, 
        tagFilter: selectedTag 
      });
    } catch (e: any) {
      notify(e.toString(), "error");
    }
  }

  async function loadStats() {
    try {
      stats = await invoke("get_stats");
    } catch (e: any) {
      // Ignore
    }
  }

  async function loadHistory() {
    try {
      history = await invoke("get_history", { limit: 50 });
    } catch (e: any) {
      notify(e.toString(), "error");
    }
  }

  // Resets selection on connection change
  $effect(() => {
    if (connections) {
      selectedHostIndex = 0;
    }
  });

  // Search trigger
  $effect(() => {
    loadConnections();
  });

  // Environment triggers
  async function switchEnv(envName: string) {
    try {
      await invoke("set_active_env", { name: envName });
      await loadData();
      notify(`Switched to profile '${envName}'`, "success");
    } catch (e: any) {
      notify(e.toString(), "error");
    }
  }

  async function createEnv() {
    if (!newEnvName.trim()) return;
    try {
      await invoke("create_environment", { name: newEnvName.trim() });
      newEnvName = "";
      showEnvModal = false;
      await loadData();
      notify("Profile created successfully", "success");
    } catch (e: any) {
      notify(e.toString(), "error");
    }
  }

  async function deleteEnv(envName: string) {
    if (confirm(`Are you sure you want to delete profile '${envName}'? All hosts in it will be lost.`)) {
      try {
        await invoke("remove_environment", { name: envName });
        await loadData();
        notify(`Profile '${envName}' deleted`, "success");
      } catch (e: any) {
        notify(e.toString(), "error");
      }
    }
  }

  // File Picker
  async function browseKey() {
    try {
      const selected = await invoke<string | null>("pick_key_file");
      if (selected) {
        modalKeyPath = selected;
      }
    } catch (e: any) {
      notify(e.toString(), "error");
    }
  }

  // Connection CRUD
  function openAddModal() {
    isEditing = false;
    modalConnectionId = "";
    modalName = "";
    modalHost = "";
    modalUser = "";
    modalPort = 22;
    modalUseKerberos = false;
    modalBastion = "";
    modalBastionUser = "";
    modalKeyPath = "";
    modalTagsString = "";
    showModal = true;
  }

  function openEditModal(conn: Connection) {
    isEditing = true;
    modalConnectionId = conn.id;
    modalName = conn.name;
    modalHost = conn.host;
    modalUser = conn.user;
    modalPort = conn.port;
    modalUseKerberos = conn.use_kerberos;
    modalBastion = conn.bastion || "";
    modalBastionUser = conn.bastion_user || "";
    modalKeyPath = conn.key_path || "";
    modalTagsString = conn.tags.join(", ");
    showModal = true;
  }

  async function saveConnection() {
    if (!modalName.trim() || !modalHost.trim()) {
      notify("Name and Host are required.", "error");
      return;
    }

    const tags = modalTagsString
      .split(",")
      .map(t => t.trim())
      .filter(t => t.length > 0);

    const payload = {
      name: modalName.trim(),
      host: modalHost.trim(),
      user: modalUser.trim() || null,
      port: modalPort || null,
      kerberos: modalUseKerberos || null,
      bastion: modalBastion.trim() || null,
      bastionUser: modalBastionUser.trim() || null,
      keyPath: modalKeyPath.trim() || null,
      tags
    };

    try {
      if (isEditing) {
        await invoke("edit_connection", { id: modalConnectionId, ...payload, user: modalUser, port: modalPort, kerberos: modalUseKerberos });
        notify("Host updated successfully", "success");
      } else {
        await invoke("add_connection", payload);
        notify("Host added successfully", "success");
      }
      showModal = false;
      await loadConnections();
      await loadStats();
    } catch (e: any) {
      notify(e.toString(), "error");
    }
  }

  async function deleteConnection(conn: Connection) {
    if (confirm(`Are you sure you want to remove connection '${conn.name}'?`)) {
      try {
        await invoke("remove_connection", { idOrName: conn.id });
        notify(`Host '${conn.name}' removed`, "success");
        await loadConnections();
        await loadStats();
      } catch (e: any) {
        notify(e.toString(), "error");
      }
    }
  }

  // Interactive Terminal Connections (PTY Integration)
  async function connectSSH(conn: Connection) {
    const tabId = Math.random().toString(36).substring(2);
    const newTab: TerminalTab = {
      id: tabId,
      name: conn.name,
      connectionName: conn.name
    };

    terminalTabs = [...terminalTabs, newTab];
    activeTerminalTabId = tabId;
    activeTab = "terminals";

    await tick();
    
    // Initialize Xterm.js
    const container = document.getElementById(`terminal-${tabId}`);
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "JetBrains Mono, Courier New, monospace",
      fontSize: 13,
      lineHeight: 1.25,
      theme: {
        background: "#0c0d12",
        foreground: "#cbd5e1",
        cursor: "#00f0ff",
        cursorAccent: "#0c0d12",
        cyan: "#00f0ff",
        magenta: "#d946ef",
        green: "#10b981",
        red: "#ef4444"
      }
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);
    fitAddon.fit();

    // Link tab
    const tabIndex = terminalTabs.findIndex(t => t.id === tabId);
    if (tabIndex !== -1) {
      terminalTabs[tabIndex].term = term;
      terminalTabs[tabIndex].fitAddon = fitAddon;
    }

    term.writeln("\x1b[1;36mInitializing Bayesian-SSH Shell Session to " + conn.name + "... \x1b[0m");

    try {
      // Spawn backend PTY
      await invoke("spawn_pty", { 
        sessionId: tabId, 
        connectionName: conn.name 
      });

      // Handle user key inputs
      term.onData(data => {
        invoke("write_pty", { sessionId: tabId, data });
      });

      // Setup window resize listener
      window.addEventListener("resize", () => {
        fitAddon.fit();
        invoke("resize_pty", { 
          sessionId: tabId, 
          cols: term.cols, 
          rows: term.rows 
        });
      });
      
    } catch (e: any) {
      term.writeln("\n\x1b[1;31mError spawning terminal process: " + e.toString() + "\x1b[0m");
    }
  }

  async function disconnectTab(tabId: string) {
    try {
      await invoke("close_pty", { sessionId: tabId });
    } catch (e) {
      // Ignore
    }
    
    const tabIndex = terminalTabs.findIndex(t => t.id === tabId);
    if (tabIndex !== -1) {
      terminalTabs[tabIndex].term?.dispose();
    }
    
    terminalTabs = terminalTabs.filter(t => t.id !== tabId);
    if (activeTerminalTabId === tabId) {
      activeTerminalTabId = terminalTabs.length > 0 ? terminalTabs[0].id : null;
      if (!activeTerminalTabId) {
        activeTab = "connections";
      }
    }
  }

  // Keyboard Shortcuts Navigation
  function handleGlobalKeydown(e: KeyboardEvent) {
    if (showModal || showEnvModal) {
      if (e.key === "Escape") {
        showModal = false;
        showEnvModal = false;
      }
      return;
    }

    // ctrl+k or '/' to focus search input
    if ((e.ctrlKey && e.key === "k") || (e.key === "/" && document.activeElement?.tagName !== "INPUT")) {
      e.preventDefault();
      const searchInput = document.querySelector(".search-input") as HTMLInputElement;
      searchInput?.focus();
      searchInput?.select();
      return;
    }

    if (activeTab === "connections" && connections.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        selectedHostIndex = (selectedHostIndex + 1) % connections.length;
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        selectedHostIndex = (selectedHostIndex - 1 + connections.length) % connections.length;
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (connections[selectedHostIndex]) {
          connectSSH(connections[selectedHostIndex]);
        }
      } else if (e.key === "e" && e.ctrlKey) {
        e.preventDefault();
        if (connections[selectedHostIndex]) {
          openEditModal(connections[selectedHostIndex]);
        }
      }
    }
  }

  onMount(() => {
    loadData();
    window.addEventListener("keydown", handleGlobalKeydown);

    // Listen for PTY output events
    const unlistenOutput = listen("pty-output", (event: any) => {
      const payload = event.payload as { session_id: string; data: string };
      const tab = terminalTabs.find(t => t.id === payload.session_id);
      if (tab?.term) {
        tab.term.write(payload.data);
      }
    });

    // Listen for process exit events
    const unlistenExit = listen("pty-exit", (event: any) => {
      const session_id = event.payload as string;
      const tab = terminalTabs.find(t => t.id === session_id);
      if (tab?.term) {
        tab.term.writeln("\n\x1b[1;33mSession disconnected.\x1b[0m");
      }
      setTimeout(() => {
        disconnectTab(session_id);
      }, 800);
      loadHistory();
      loadStats();
    });

    return () => {
      window.removeEventListener("keydown", handleGlobalKeydown);
      unlistenOutput.then(fn => fn());
      unlistenExit.then(fn => fn());
    };
  });

  // Extract tags
  const allTags = $derived.by(() => {
    const tagsSet = new Set<string>();
    connections.forEach(c => c.tags.forEach(t => tagsSet.add(t)));
    return Array.from(tagsSet).sort();
  });
</script>

<div class="window-container">
  <!-- VS Code inspired custom Titlebar -->
  <header class="custom-titlebar" data-tauri-drag-region>
    <!-- Left: App Logo, Title, Menu -->
    <div class="titlebar-left" data-tauri-drag-region style="display: flex; align-items: center;">
      <div class="app-logo" style="display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; margin-right: 8px;">
        <svg viewBox="0 0 1024 1024" style="width: 16px; height: 16px; fill: none; stroke: var(--accent-cyan); stroke-width: 80;">
          <path d="M100 800 C300 800, 400 200, 512 200 C624 200, 724 800, 924 800" />
          <line x1="512" y1="200" x2="512" y2="800" stroke="var(--accent-pink)" />
        </svg>
      </div>
      <span class="titlebar-app-name" style="font-size: 12px; font-weight: 500; color: var(--text-primary); margin-right: 16px; user-select: none;">Bayesian SSH</span>
      
      <!-- Dummy Menu items matching VS Code style -->
      <div class="titlebar-menu" style="display: flex; gap: 8px;">
        <span class="menu-item">File</span>
        <span class="menu-item">Edit</span>
        <span class="menu-item">Selection</span>
        <span class="menu-item">Terminal</span>
        <span class="menu-item">Help</span>
      </div>
    </div>

    <!-- Center: Path/Profile Indicator -->
    <div class="titlebar-center" data-tauri-drag-region>
      <div class="titlebar-search-bar" style="background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 4px; padding: 4px 16px; font-size: 11px; color: var(--text-secondary); display: flex; align-items: center; gap: 6px; user-select: none;">
        <Server size={10} style="color: var(--accent-cyan);" />
        <span>Bayesian SSH &mdash; Profile: <strong style="color: var(--text-primary);">{activeEnv}</strong></span>
      </div>
    </div>

    <!-- Right: Window controls -->
    <div class="titlebar-right" style="display: flex; align-items: center;">
      <button class="win-ctrl-btn minimize" onclick={handleWindowMinimize} title="Minimize">
        <svg viewBox="0 0 10 1" style="width: 10px; height: 1px; fill: none; stroke: currentColor; stroke-width: 1.5;"><line x1="0" y1="0.5" x2="10" y2="0.5" /></svg>
      </button>
      <button class="win-ctrl-btn maximize" onclick={handleWindowMaximize} title="Maximize/Restore">
        <svg viewBox="0 0 10 10" style="width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1.2;"><rect x="1" y="1" width="8" height="8" /></svg>
      </button>
      <button class="win-ctrl-btn close" onclick={handleWindowClose} title="Close">
        <svg viewBox="0 0 10 10" style="width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1.2;"><path d="M1 1 L9 9 M9 1 L1 9" /></svg>
      </button>
    </div>
  </header>

  <div class="app-layout" class:collapsed={sidebarCollapsed}>
  <!-- SIDEBAR -->
  <aside class="sidebar">
    <div class="logo-container">
      <TerminalSquare class="logo-icon" size={24} />
      <span class="logo-text">BSSH</span>
    </div>

    <!-- Active Environment / Profile Picker -->
    <div class="env-widget">
      <div class="env-header">
        <span class="section-title">PROFILE</span>
        <button class="small-icon-btn" onclick={() => showEnvModal = true} title="Manage Profiles">
          <FolderPlus size={14} />
        </button>
      </div>
      <div class="select-wrapper">
        <select class="cyber-select" value={activeEnv} onchange={(e) => switchEnv((e.target as HTMLSelectElement).value)}>
          {#each environments as env}
            <option value={env.name}>{env.name}</option>
          {/each}
        </select>
      </div>
    </div>

    <!-- Navigation Menu -->
    <nav class="nav-menu">
      <button class="nav-item" class:active={activeTab === 'connections'} onclick={() => activeTab = 'connections'}>
        <Server size={18} />
        <span class="nav-label">Hosts</span>
      </button>
      <button class="nav-item" class:active={activeTab === 'terminals'} onclick={() => activeTab = 'terminals'}>
        <TerminalSquare size={18} />
        <span class="nav-label">Terminals</span>
        {#if terminalTabs.length > 0}
          <span class="badge">{terminalTabs.length}</span>
        {/if}
      </button>
      <button class="nav-item" class:active={activeTab === 'history'} onclick={() => activeTab = 'history'}>
        <Clock size={18} />
        <span class="nav-label">Logs</span>
      </button>
      <button class="nav-item" class:active={activeTab === 'settings'} onclick={() => activeTab = 'settings'}>
        <Settings size={18} />
        <span class="nav-label">Settings</span>
      </button>
    </nav>

    <!-- Sidebar Stats -->
    {#if stats && !sidebarCollapsed}
      <div class="sidebar-stats">
        <span class="section-title">METRICS</span>
        <div class="stat-row">
          <span>Registered:</span>
          <span>{stats.total_connections} hosts</span>
        </div>
        {#if stats.most_used}
          <div class="stat-row" onclick={() => searchQuery = stats?.most_used?.name || ""}>
            <span>Frequent:</span>
            <span class="text-glow">{stats.most_used.name}</span>
          </div>
        {/if}
      </div>
    {/if}

    <!-- SSH Agent Manager -->
    {#if !sidebarCollapsed}
      <div class="sidebar-stats">
        <span class="section-title">SSH AGENT</span>
        <div class="stat-row">
          <span>Status:</span>
          {#if agentActive}
            <span class="status-indicator active" onclick={() => showAgentModal = true} style="cursor: pointer; display: flex; align-items: center; gap: 6px;">
              <span class="dot" style="width: 8px; height: 8px; border-radius: 50%; background-color: var(--accent-cyan); display: inline-block; box-shadow: 0 0 8px var(--accent-cyan);"></span> Active
            </span>
          {:else}
            <button class="agent-start-btn" onclick={triggerStartAgent} style="background: none; border: none; color: var(--text-muted); cursor: pointer; display: flex; align-items: center; gap: 6px; padding: 0; font-size: 11px;">
              <span class="dot" style="width: 8px; height: 8px; border-radius: 50%; background-color: var(--text-muted); display: inline-block;"></span> Start Agent
            </button>
          {/if}
        </div>
        {#if agentActive}
          <div class="stat-row clickable" onclick={() => showAgentModal = true} style="cursor: pointer;">
            <span>Loaded Keys:</span>
            <span class="text-glow" style="color: var(--accent-cyan);">{agentKeys.length} keys</span>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Quick Tags -->
    {#if allTags.length > 0 && !sidebarCollapsed}
      <div class="quick-tags">
        <span class="section-title">TAGS</span>
        <div class="tags-list">
          <button class="tag-btn" class:active={selectedTag === null} onclick={() => { selectedTag = null; loadConnections(); }}>
            All
          </button>
          {#each allTags as tag}
            <button class="tag-btn" class:active={selectedTag === tag} onclick={() => { selectedTag = tag; loadConnections(); }}>
              #{tag}
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Sidebar Toggle -->
    <button class="sidebar-toggle-btn" onclick={() => sidebarCollapsed = !sidebarCollapsed}>
      {#if sidebarCollapsed}
        <ChevronRight size={16} />
      {:else}
        <ChevronLeft size={16} />
      {/if}
    </button>
  </aside>

  <!-- MAIN VIEWPORT -->
  <main class="main-panel">
    <!-- Top Bar -->
    <header class="topbar">
      <div class="search-section">
        <div class="search-container">
          <Search class="search-icon" size={16} />
          <input 
            type="text" 
            placeholder="Search host, alias, or tag... (Press '/' to focus)" 
            bind:value={searchQuery}
            class="search-input"
          />
        </div>
        
        <!-- View mode toggles -->
        {#if activeTab === 'connections'}
          <div class="view-mode-toggle">
            <button class="toggle-btn" class:active={viewMode === 'list'} onclick={() => viewMode = 'list'} title="List View">
              <List size={16} />
            </button>
            <button class="toggle-btn" class:active={viewMode === 'grid'} onclick={() => viewMode = 'grid'} title="Grid View">
              <LayoutGrid size={16} />
            </button>
          </div>
        {/if}
      </div>

      <div class="topbar-actions">
        <button class="cyber-btn" onclick={openAddModal}>
          <Plus size={16} />
          <span>New Server</span>
        </button>
      </div>
    </header>

    <!-- Content View -->
    <div class="content-body">
      <!-- 1. Connections Tab -->
      {#if activeTab === 'connections'}
        <div class="connections-view">
          <div class="view-header">
            <div class="title-meta">
              <h2>SSH Connections</h2>
              <span class="subtitle">Bayesian ranked hosts based on connection frequency and recency</span>
            </div>
            <button class="refresh-btn" onclick={loadConnections} title="Refresh Connections">
              <RefreshCw size={14} />
            </button>
          </div>

          {#if connections.length > 0}
            <!-- 1.a. Linear-Style List View -->
            {#if viewMode === 'list'}
              <div class="list-container">
                <div class="list-header">
                  <div class="col-name">Name</div>
                  <div class="col-host">Address</div>
                  <div class="col-tags">Tags</div>
                  <div class="col-last">Last Session</div>
                  <div class="col-actions"></div>
                </div>

                <div class="list-body">
                  {#each connections as conn, index}
                    <div 
                      class="list-row" 
                      class:selected={selectedHostIndex === index}
                      onclick={() => selectedHostIndex = index}
                      ondblclick={() => connectSSH(conn)}
                      role="row"
                      tabindex="0"
                      onkeydown={(e) => e.key === 'Enter' && connectSSH(conn)}
                    >
                      <!-- Name column -->
                      <div class="col-name">
                        <Server size={14} class="row-icon" />
                        <span class="host-name">{conn.name}</span>
                        {#if conn.use_kerberos}
                          <span class="row-badge krb" title="Kerberos authentication enabled">krb5</span>
                        {/if}
                      </div>

                      <!-- Host address column -->
                      <div class="col-host font-mono">
                        {conn.user}@{conn.host}:{conn.port}
                      </div>

                      <!-- Tags column -->
                      <div class="col-tags">
                        {#each conn.tags as tag}
                          <span class="tag-pill">#{tag}</span>
                        {/each}
                      </div>

                      <!-- Last connected -->
                      <div class="col-last text-muted">
                        {conn.last_used ? new Date(conn.last_used).toLocaleDateString() : 'Never'}
                      </div>

                      <!-- Action buttons on hover -->
                      <div class="col-actions">
                        <button class="row-action-btn copy" onclick={(e) => { e.stopPropagation(); copyToClipboard(conn.to_ssh_command(), conn.id); }} title="Copy SSH command">
                          {#if copiedId === conn.id}
                            <Check size={14} />
                          {:else}
                            <Copy size={14} />
                          {/if}
                        </button>
                        <button class="row-action-btn edit" onclick={(e) => { e.stopPropagation(); openEditModal(conn); }} title="Edit">
                          <Edit2 size={14} />
                        </button>
                        <button class="row-action-btn delete" onclick={(e) => { e.stopPropagation(); deleteConnection(conn); }} title="Delete">
                          <Trash2 size={14} />
                        </button>
                        <button class="row-action-btn connect" onclick={(e) => { e.stopPropagation(); connectSSH(conn); }} title="Connect">
                          <Play size={12} fill="currentColor" />
                        </button>
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}

            <!-- 1.b. Elegant Cards Grid View -->
            {#if viewMode === 'grid'}
              <div class="hosts-grid">
                {#each connections as conn, index}
                  <div 
                    class="host-card" 
                    class:selected={selectedHostIndex === index}
                    onclick={() => selectedHostIndex = index}
                    ondblclick={() => connectSSH(conn)}
                    role="presentation"
                  >
                    <div class="card-left-accent"></div>
                    <div class="card-content">
                      <div class="card-top">
                        <div class="card-title-block">
                          <h4>{conn.name}</h4>
                          <span class="host-addr font-mono">{conn.user}@{conn.host}:{conn.port}</span>
                        </div>
                        {#if conn.use_kerberos}
                          <span class="card-krb-badge"><Shield size={10} /> KRB</span>
                        {/if}
                      </div>

                      <div class="card-tags-row">
                        {#each conn.tags as tag}
                          <span class="tag-pill">#{tag}</span>
                        {/each}
                      </div>

                      <div class="card-footer">
                        <span class="last-used">
                          {conn.last_used ? `Used ${new Date(conn.last_used).toLocaleDateString()}` : 'Unused'}
                        </span>
                        
                        <div class="card-btns">
                          <button class="card-btn-icon" onclick={(e) => { e.stopPropagation(); copyToClipboard(conn.to_ssh_command(), conn.id); }} title="Copy command">
                            {#if copiedId === conn.id}
                              <Check size={12} />
                            {:else}
                              <Copy size={12} />
                            {/if}
                          </button>
                          <button class="card-btn-icon" onclick={(e) => { e.stopPropagation(); openEditModal(conn); }} title="Edit">
                            <Edit2 size={12} />
                          </button>
                          <button class="card-btn-icon" onclick={(e) => { e.stopPropagation(); deleteConnection(conn); }} title="Delete">
                            <Trash2 size={12} />
                          </button>
                          <button class="card-connect-btn-small" onclick={(e) => { e.stopPropagation(); connectSSH(conn); }}>
                            <Play size={10} fill="currentColor" /> Connect
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {:else}
            <div class="empty-state">
              <Server size={36} class="empty-icon" />
              <h3>No servers configured</h3>
              <p>Setup a connection profile to start managing your sessions</p>
              <button class="cyber-btn" onclick={openAddModal}>Add Host Connection</button>
            </div>
          {/if}
        </div>
      {/if}

      <!-- 2. Terminals Tab -->
      {#if activeTab === 'terminals'}
        <div class="terminals-view">
          {#if terminalTabs.length > 0}
            <!-- Tabs -->
            <div class="terminal-tabs">
              {#each terminalTabs as tab}
                <div 
                  class="terminal-tab-btn" 
                  class:active={activeTerminalTabId === tab.id}
                  onclick={() => activeTerminalTabId = tab.id}
                  role="button"
                  tabindex="0"
                  onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') activeTerminalTabId = tab.id; }}
                >
                  <Server size={12} />
                  <span>{tab.name}</span>
                  <button class="tab-close-btn" onclick={(e) => { e.stopPropagation(); disconnectTab(tab.id); }}>
                    <X size={12} />
                  </button>
                </div>
              {/each}
            </div>

            <!-- Viewports -->
            <div class="terminal-viewport-container">
              {#each terminalTabs as tab}
                <div 
                  id="terminal-{tab.id}" 
                  class="terminal-viewport" 
                  class:hidden={activeTerminalTabId !== tab.id}
                ></div>
              {/each}
            </div>
          {:else}
            <div class="empty-state">
              <TerminalSquare size={36} class="empty-icon" />
              <h3>No terminal sessions</h3>
              <p>Spawn an interactive SSH session from your list of hosts</p>
              <button class="cyber-btn" onclick={() => activeTab = 'connections'}>Back to Hosts</button>
            </div>
          {/if}
        </div>
      {/if}

      <!-- 3. Logs Tab -->
      {#if activeTab === 'history'}
        <div class="history-view">
          <div class="view-header">
            <div class="title-meta">
              <h2>Session Logs</h2>
              <span class="subtitle">Complete audit logs of connection sessions</span>
            </div>
          </div>

          <div class="table-container">
            <table class="history-table">
              <thead>
                <tr>
                  <th>Host Connection</th>
                  <th>Started At</th>
                  <th>Ended At</th>
                  <th>Status</th>
                  <th>Exit Code</th>
                </tr>
              </thead>
              <tbody>
                {#each history as entry}
                  <tr>
                    <td class="font-semibold text-white">{entry.name}</td>
                    <td>{new Date(entry.started_at).toLocaleString()}</td>
                    <td>
                      {entry.ended_at ? new Date(entry.ended_at).toLocaleString() : 'Active/Stale'}
                    </td>
                    <td>
                      {#if entry.status.includes("Connected") || entry.status.includes("Completed") || entry.status.includes("Active")}
                        <span class="status-badge success">
                          <CheckCircle2 size={12} /> Active
                        </span>
                      {:else}
                        <span class="status-badge error">
                          <AlertCircle size={12} /> Failed
                        </span>
                      {/if}
                    </td>
                    <td class="font-mono">
                      {entry.exit_code !== undefined ? entry.exit_code : '-'}
                    </td>
                  </tr>
                {:else}
                  <tr>
                    <td colspan="5" style="text-align: center; padding: 3rem; color: var(--text-muted);">
                      No historical logs found.
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      {/if}

      <!-- 4. Settings Tab -->
      {#if activeTab === 'settings'}
        <div class="settings-view" style="padding: 24px; color: var(--text-primary); display: flex; flex-direction: column; gap: 24px; overflow-y: auto; height: 100%;">
          <div class="view-header" style="border-bottom: 1px solid var(--border-color); padding-bottom: 16px; margin-bottom: 8px;">
            <div class="title-meta">
              <h2 style="font-size: 20px; font-weight: 600; margin: 0 0 4px 0;">Desktop Settings</h2>
              <span class="subtitle" style="color: var(--text-muted); font-size: 12px;">Configure preferences for the desktop application</span>
            </div>
          </div>

          <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 24px;">
            <!-- Column 1: Appearance & UI -->
            <div style="background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 8px; padding: 20px; display: flex; flex-direction: column; gap: 16px;">
              <span style="font-size: 11px; font-weight: 700; letter-spacing: 0.05em; color: var(--text-muted);">APPEARANCE</span>
              
              <div class="form-group" style="display: flex; flex-direction: column; gap: 6px;">
                <label style="font-size: 12px; font-weight: 500;">Active Theme</label>
                <select class="cyber-select" bind:value={settings.theme} onchange={saveSettings} style="width: 100%; box-sizing: border-box; background: var(--bg-app); border: 1px solid var(--border-color); border-radius: 6px; color: var(--text-primary); padding: 8px 12px; font-size: 13px;">
                  <option value="zinc">Slate Minimalist (Zinc)</option>
                  <option value="cyberpunk">Cyberpunk Neon (Dark Glow)</option>
                  <option value="oled">OLED Pitch Black</option>
                  <option value="slate">Sleek Navy (Slate)</option>
                </select>
              </div>

              <div class="form-group" style="display: flex; align-items: center; justify-content: space-between; margin-top: 12px;">
                <div style="display: flex; flex-direction: column; gap: 2px;">
                  <label style="font-size: 12px; font-weight: 500;">Fuzzy Search Scoring</label>
                  <span style="font-size: 10px; color: var(--text-muted);">Prioritize query match over Bayesian connect frequency</span>
                </div>
                <input type="checkbox" bind:checked={settings.fuzzy_search} onchange={saveSettings} style="width: 16px; height: 16px; accent-color: var(--accent-cyan);" />
              </div>
            </div>

            <!-- Column 2: SSH Agent Config -->
            <div style="background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 8px; padding: 20px; display: flex; flex-direction: column; gap: 16px;">
              <span style="font-size: 11px; font-weight: 700; letter-spacing: 0.05em; color: var(--text-muted);">SSH AGENT INTEGRATION</span>

              <div class="form-group" style="display: flex; align-items: center; justify-content: space-between;">
                <div style="display: flex; flex-direction: column; gap: 2px;">
                  <label style="font-size: 12px; font-weight: 500;">Auto-start SSH Agent</label>
                  <span style="font-size: 10px; color: var(--text-muted);">Start agent automatically on desktop app startup</span>
                </div>
                <input type="checkbox" bind:checked={settings.auto_start_agent} onchange={saveSettings} style="width: 16px; height: 16px; accent-color: var(--accent-cyan);" />
              </div>

              <div class="form-group" style="display: flex; flex-direction: column; gap: 6px;">
                <label style="font-size: 12px; font-weight: 500;">Custom Agent Socket Path</label>
                <input type="text" placeholder="e.g. /tmp/custom-agent.sock (blank to use default)" bind:value={settings.custom_agent_socket} onchange={saveSettings} class="cyber-input" style="width: 100%; box-sizing: border-box;" />
              </div>
            </div>

            <!-- Column 3: Defaults -->
            <div style="background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 8px; padding: 20px; display: flex; flex-direction: column; gap: 16px;">
              <span style="font-size: 11px; font-weight: 700; letter-spacing: 0.05em; color: var(--text-muted);">CONNECTION DEFAULTS</span>

              <div class="form-group" style="display: flex; flex-direction: column; gap: 6px;">
                <label style="font-size: 12px; font-weight: 500;">Default SSH User</label>
                <input type="text" bind:value={settings.default_user} onchange={saveSettings} class="cyber-input" style="width: 100%; box-sizing: border-box;" />
              </div>

              <div class="form-group" style="display: flex; flex-direction: column; gap: 6px;">
                <label style="font-size: 12px; font-weight: 500;">Default SSH Port</label>
                <input type="number" bind:value={settings.default_port} onchange={saveSettings} class="cyber-input" style="width: 100%; box-sizing: border-box;" />
              </div>
            </div>
          </div>
        </div>
      {/if}
    </div>
  </main>
</div>

<!-- CONNECT MODAL (SLIDE-OVER DRAWER) -->
{#if showModal}
  <div class="modal-backdrop" onclick={() => showModal = false}>
    <div class="modal-dialog" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>{isEditing ? 'Edit Connection' : 'New SSH Connection'}</h2>
        <button class="close-btn" onclick={() => showModal = false}><X size={18} /></button>
      </div>

      <div class="modal-body">
        <div class="form-row">
          <div class="form-group flex-2">
            <label for="c-name">Connection Name</label>
            <input id="c-name" type="text" placeholder="e.g. Server Production" bind:value={modalName} class="cyber-input" />
          </div>
          <div class="form-group flex-3">
            <label for="c-host">Hostname / IP Address</label>
            <input id="c-host" type="text" placeholder="e.g. 192.168.1.50" bind:value={modalHost} class="cyber-input" />
          </div>
        </div>

        <div class="form-row">
          <div class="form-group">
            <label for="c-user">SSH Username</label>
            <input id="c-user" type="text" placeholder="root" bind:value={modalUser} class="cyber-input" />
          </div>
          <div class="form-group">
            <label for="c-port">Port</label>
            <input id="c-port" type="number" bind:value={modalPort} class="cyber-input" />
          </div>
        </div>

        <div class="form-group">
          <label for="c-key">Identity File (SSH Private Key)</label>
          <div class="input-with-action">
            <input id="c-key" type="text" placeholder="Path to SSH key file" bind:value={modalKeyPath} class="cyber-input" />
            <button class="browse-btn" onclick={browseKey}><Key size={14} /> Browse</button>
          </div>
        </div>

        <div class="checkbox-row" onclick={() => modalUseKerberos = !modalUseKerberos} role="presentation">
          <input id="c-krb" type="checkbox" bind:checked={modalUseKerberos} onclick={(e) => e.stopPropagation()} />
          <label for="c-krb">Enable Kerberos / GSSAPI Authentication</label>
        </div>

        <div class="form-section-divider">
          <span>BASTION JUMP HOST (OPTIONAL)</span>
        </div>

        <div class="form-row">
          <div class="form-group">
            <label for="c-bastion">Bastion Address</label>
            <input id="c-bastion" type="text" placeholder="bastion.internal" bind:value={modalBastion} class="cyber-input" />
          </div>
          <div class="form-group">
            <label for="c-bastion-user">Bastion Username</label>
            <input id="c-bastion-user" type="text" placeholder="jumpuser" bind:value={modalBastionUser} class="cyber-input" />
          </div>
        </div>

        <div class="form-group">
          <label for="c-tags">Tags (Separated by commas)</label>
          <input id="c-tags" type="text" placeholder="e.g. backend, aws, production" bind:value={modalTagsString} class="cyber-input" />
        </div>
      </div>

      <div class="modal-footer">
        <button class="cancel-btn" onclick={() => showModal = false}>Cancel</button>
        <button class="save-btn" onclick={saveConnection}>Save Server</button>
      </div>
    </div>
  </div>
{/if}

<!-- ENVIRONMENT MODAL -->
{#if showEnvModal}
  <div class="modal-backdrop" onclick={() => showEnvModal = false}>
    <div class="modal-dialog mini" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>Manage Profiles</h2>
        <button class="close-btn" onclick={() => showEnvModal = false}><X size={18} /></button>
      </div>

      <div class="modal-body">
        <div class="form-group">
          <label for="e-name">New Profile Name</label>
          <input id="e-name" type="text" placeholder="e.g. Dev-Local" bind:value={newEnvName} class="cyber-input" />
        </div>

        <div class="profile-manager-list">
          <span class="section-title">ACTIVE PROFILES</span>
          <div class="profile-rows">
            {#each environments as env}
              <div class="profile-row">
                <span class="profile-name" class:active={env.is_active}>{env.name}</span>
                {#if env.name !== 'default' && !env.is_active}
                  <button class="profile-delete-btn" onclick={() => deleteEnv(env.name)} title="Delete Profile">
                    <Trash2 size={12} />
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button class="cancel-btn" onclick={() => showEnvModal = false}>Close</button>
        <button class="save-btn" onclick={createEnv}>Add Profile</button>
      </div>
    </div>
  </div>
{/if}

<!-- SSH AGENT MODAL -->
{#if showAgentModal}
  <div class="modal-backdrop" onclick={() => showAgentModal = false}>
    <div class="modal-dialog" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>SSH Agent Manager</h2>
        <button class="close-btn" onclick={() => showAgentModal = false}><X size={18} /></button>
      </div>

      <div class="modal-body">
        <div class="agent-info-box" style="background: var(--bg-card); padding: 12px; border: 1px solid var(--border-color); border-radius: 6px; margin-bottom: 16px;">
          <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
            <span style="font-weight: 500; font-size: 13px; color: var(--text-color);">AGENT STATUS</span>
            <span style="font-size: 11px; font-weight: 600; color: var(--accent-cyan);">ACTIVE</span>
          </div>
          {#if agentSocket}
            <div style="font-size: 11px; color: var(--text-muted); word-break: break-all; font-family: monospace;">
              Socket: {agentSocket}
            </div>
          {/if}
        </div>

        <div class="agent-keys-section">
          <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
            <span class="section-title" style="margin: 0;">LOADED KEYS ({agentKeys.length})</span>
            <button class="cyber-btn mini" onclick={selectAndAddKey} style="padding: 4px 8px; font-size: 11px; display: flex; align-items: center; gap: 4px;">
              <Plus size={12} /> Add Key File
            </button>
          </div>

          <div class="keys-list-container" style="max-height: 200px; overflow-y: auto; border: 1px solid var(--border-color); border-radius: 6px; background: var(--bg-card);">
            {#if agentKeys.length === 0}
              <div style="padding: 24px; text-align: center; color: var(--text-muted); font-size: 12px;">
                No keys currently loaded in the SSH Agent.
              </div>
            {:else}
              {#each agentKeys as key}
                <div class="key-item" style="padding: 10px 12px; border-bottom: 1px solid var(--border-color); font-family: monospace; font-size: 11px; color: var(--text-color); display: flex; justify-content: space-between; align-items: center;">
                  <span style="word-break: break-all; margin-right: 8px;">{key}</span>
                </div>
              {/each}
            {/if}
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button class="cancel-btn" onclick={() => showAgentModal = false}>Close</button>
      </div>
    </div>
  </div>
{/if}

<!-- NOTIFICATION -->
{#if showNotification}
  <div class="toast-notification" class:success={notificationType === 'success'} class:error={notificationType === 'error'}>
    <span>{notificationText}</span>
  </div>
{/if}
</div>

<style>
  /* ---------------------------------------------------- */
  /* DESIGN SYSTEM: Vercel / Linear inspired developer GUI */
  /* ---------------------------------------------------- */

  :root {
    --bg-app: #09090b;       /* Zinc 950 */
    --bg-sidebar: #09090b;   /* Seamless zinc background */
    --bg-card: #18181b;      /* Zinc 900 */
    --bg-card-hover: #27272a;/* Zinc 800 */
    --border-color: rgba(255, 255, 255, 0.06);
    --border-color-hover: rgba(255, 255, 255, 0.15);
    
    --text-primary: #f4f4f5; /* Zinc 100 */
    --text-secondary: #a1a1aa;/* Zinc 400 */
    --text-muted: #71717a;    /* Zinc 500 */
    
    --accent-cyan: #00f0ff;
    --accent-pink: #d946ef;
    
    --green-emerald: #10b981;
    --red-rose: #f43f5e;
  }

  :global(.theme-zinc) {
    --bg-app: #09090b;
    --bg-sidebar: #09090b;
    --bg-card: #18181b;
    --bg-card-hover: #27272a;
    --border-color: rgba(255, 255, 255, 0.06);
    --border-color-hover: rgba(255, 255, 255, 0.15);
    --text-primary: #f4f4f5;
    --text-secondary: #a1a1aa;
    --text-muted: #71717a;
    --accent-cyan: #00f0ff;
    --accent-pink: #d946ef;
  }

  :global(.theme-cyberpunk) {
    --bg-app: #0c0813;
    --bg-sidebar: #06040a;
    --bg-card: #140d24;
    --bg-card-hover: #21153b;
    --border-color: rgba(0, 240, 255, 0.15);
    --border-color-hover: rgba(217, 70, 239, 0.3);
    --text-primary: #f8fafc;
    --text-secondary: #c084fc;
    --text-muted: #7a1a75;
    --accent-cyan: #00f0ff;
    --accent-pink: #d946ef;
  }

  :global(.theme-oled) {
    --bg-app: #000000;
    --bg-sidebar: #000000;
    --bg-card: #09090b;
    --bg-card-hover: #18181b;
    --border-color: rgba(255, 255, 255, 0.04);
    --border-color-hover: rgba(255, 255, 255, 0.12);
    --text-primary: #ffffff;
    --text-secondary: #d4d4d8;
    --text-muted: #52525b;
    --accent-cyan: #00f0ff;
    --accent-pink: #d946ef;
  }

  :global(.theme-slate) {
    --bg-app: #0f172a;
    --bg-sidebar: #0f172a;
    --bg-card: #1e293b;
    --bg-card-hover: #334155;
    --border-color: rgba(255, 255, 255, 0.05);
    --border-color-hover: rgba(255, 255, 255, 0.12);
    --text-primary: #f8fafc;
    --text-secondary: #cbd5e1;
    --text-muted: #64748b;
    --accent-cyan: #38bdf8;
    --accent-pink: #f43f5e;
  }

  :global(body) {
    background-color: var(--bg-app);
    color: var(--text-primary);
    margin: 0;
    font-family: 'Outfit', 'Inter', sans-serif;
    overflow: hidden;
    user-select: none;
    -webkit-font-smoothing: antialiased;
  }

  .font-mono {
    font-family: 'JetBrains Mono', Courier, monospace;
    font-size: 0.85rem;
  }

  /* Custom VS Code inspired Titlebar */
  .window-container {
    display: flex;
    flex-direction: column;
    height: 100vh;
    width: 100vw;
    overflow: hidden;
  }

  .custom-titlebar {
    height: 32px;
    background-color: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-color);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 12px;
    user-select: none;
    z-index: 1000;
  }

  .titlebar-left {
    display: flex;
    align-items: center;
  }

  .titlebar-menu .menu-item {
    font-size: 11px;
    color: var(--text-secondary);
    padding: 2px 6px;
    border-radius: 4px;
    cursor: default;
    transition: background 0.15s, color 0.15s;
  }

  .titlebar-menu .menu-item:hover {
    background-color: var(--bg-card-hover);
    color: var(--text-primary);
  }

  .titlebar-center {
    flex: 1;
    display: flex;
    justify-content: center;
    max-width: 400px;
  }

  .titlebar-right {
    display: flex;
    align-items: center;
  }

  .win-ctrl-btn {
    width: 46px;
    height: 32px;
    background: none;
    border: none;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }

  .win-ctrl-btn:hover {
    background-color: var(--bg-card-hover);
    color: var(--text-primary);
  }

  .win-ctrl-btn.close:hover {
    background-color: var(--red-rose) !important;
    color: white !important;
  }

  .app-layout {
    display: flex;
    height: calc(100vh - 32px);
    width: 100vw;
    background-color: var(--bg-app);
  }

  /* SIDEBAR */
  .sidebar {
    width: 240px;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border-color);
    display: flex;
    flex-direction: column;
    padding: 1.5rem 1rem;
    box-sizing: border-box;
    position: relative;
    transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .app-layout.collapsed .sidebar {
    width: 68px;
    padding: 1.5rem 0.5rem;
  }

  .logo-container {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 2rem;
    padding-left: 0.5rem;
  }

  .app-layout.collapsed .logo-container {
    justify-content: center;
    padding-left: 0;
  }

  .logo-icon {
    color: var(--accent-cyan);
  }

  .logo-text {
    font-size: 1.1rem;
    font-weight: 800;
    letter-spacing: 1.5px;
    color: var(--text-primary);
  }

  .app-layout.collapsed .logo-text {
    display: none;
  }

  .section-title {
    font-size: 0.7rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 1px;
    color: var(--text-muted);
    margin-bottom: 0.5rem;
    display: block;
  }

  .app-layout.collapsed .section-title {
    display: none;
  }

  .env-widget {
    margin-bottom: 2rem;
  }

  .app-layout.collapsed .env-widget {
    display: none;
  }

  .env-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .small-icon-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    border-radius: 4px;
  }

  .small-icon-btn:hover {
    color: var(--text-primary);
    background: rgba(255,255,255,0.05);
  }

  .select-wrapper {
    position: relative;
    margin-top: 4px;
  }

  .cyber-select {
    width: 100%;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    padding: 0.5rem;
    border-radius: 6px;
    outline: none;
    font-size: 0.85rem;
    cursor: pointer;
    appearance: none;
  }

  .cyber-select:hover {
    border-color: var(--border-color-hover);
  }

  .nav-menu {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 2rem;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 12px;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    padding: 0.6rem 0.75rem;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.9rem;
    text-align: left;
    transition: all 0.2s;
    position: relative;
    width: 100%;
    box-sizing: border-box;
  }

  .app-layout.collapsed .nav-item {
    justify-content: center;
    padding: 0.6rem 0;
  }

  .app-layout.collapsed .nav-label {
    display: none;
  }

  .nav-item:hover, .nav-item.active {
    color: var(--text-primary);
    background: rgba(255, 255, 255, 0.05);
  }

  .nav-item.active {
    background: rgba(255, 255, 255, 0.08);
  }

  .nav-item.active::before {
    content: '';
    position: absolute;
    left: 0;
    top: 25%;
    height: 50%;
    width: 2px;
    background: var(--accent-cyan);
    border-radius: 0 4px 4px 0;
  }

  .sidebar-stats {
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    padding: 0.75rem;
    margin-bottom: 1.5rem;
  }

  .stat-row {
    display: flex;
    justify-content: space-between;
    font-size: 0.8rem;
    margin-bottom: 0.4rem;
    color: var(--text-secondary);
  }

  .quick-tags {
    margin-top: auto;
  }

  .tags-list {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .tag-btn {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    font-size: 0.75rem;
    padding: 2px 6px;
    border-radius: 4px;
    cursor: pointer;
  }

  .tag-btn:hover, .tag-btn.active {
    color: var(--accent-cyan);
    border-color: rgba(0, 240, 255, 0.3);
    background: rgba(0, 240, 255, 0.05);
  }

  .sidebar-toggle-btn {
    position: absolute;
    bottom: 1rem;
    right: -12px;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    cursor: pointer;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }

  .sidebar-toggle-btn:hover {
    color: var(--text-primary);
    border-color: var(--border-color-hover);
  }

  /* MAIN PANEL */
  .main-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
    overflow: hidden;
  }

  /* TOP BAR */
  .topbar {
    height: 56px;
    border-bottom: 1px solid var(--border-color);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 1.5rem;
    box-sizing: border-box;
  }

  .search-section {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .search-container {
    display: flex;
    align-items: center;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    padding: 0.4rem 0.75rem;
    width: 320px;
    transition: all 0.2s;
  }

  .search-container:focus-within {
    border-color: var(--border-color-hover);
    background: rgba(255, 255, 255, 0.05);
  }

  .search-icon {
    color: var(--text-muted);
    margin-right: 8px;
  }

  .search-input {
    background: transparent;
    border: none;
    color: var(--text-primary);
    outline: none;
    width: 100%;
    font-size: 0.85rem;
  }

  .view-mode-toggle {
    display: flex;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    padding: 2px;
    background: rgba(255,255,255,0.01);
  }

  .toggle-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    padding: 4px 8px;
    border-radius: 4px;
    cursor: pointer;
    display: flex;
  }

  .toggle-btn.active {
    color: var(--text-primary);
    background: rgba(255, 255, 255, 0.06);
  }

  .cyber-btn {
    background: var(--text-primary);
    border: none;
    color: var(--bg-app);
    padding: 0.45rem 1rem;
    border-radius: 6px;
    font-weight: 600;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.85rem;
    transition: opacity 0.2s;
  }

  .cyber-btn:hover {
    opacity: 0.9;
  }

  /* CONTENT BODY */
  .content-body {
    flex: 1;
    padding: 1.5rem;
    overflow-y: auto;
    box-sizing: border-box;
  }

  .view-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 1.5rem;
  }

  .view-header h2 {
    font-size: 1.25rem;
    margin: 0;
    font-weight: 700;
    letter-spacing: -0.5px;
  }

  .view-header .subtitle {
    color: var(--text-secondary);
    font-size: 0.8rem;
    margin-top: 2px;
    display: block;
  }

  .refresh-btn {
    background: transparent;
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    padding: 6px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
  }

  .refresh-btn:hover {
    border-color: var(--border-color-hover);
    color: var(--text-primary);
  }

  /* --------------------------------- */
  /* LINEAR-STYLE LIST VIEW            */
  /* --------------------------------- */
  .list-container {
    border: 1px solid var(--border-color);
    border-radius: 8px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.01);
  }

  .list-header {
    display: flex;
    background: rgba(255, 255, 255, 0.02);
    border-bottom: 1px solid var(--border-color);
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 0.75rem 1rem;
  }

  .list-row {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border-color);
    padding: 0.75rem 1rem;
    font-size: 0.85rem;
    color: var(--text-secondary);
    cursor: pointer;
    outline: none;
    transition: background 0.2s;
  }

  .list-row:last-child {
    border-bottom: none;
  }

  .list-row:hover, .list-row.selected {
    background: rgba(255, 255, 255, 0.03);
    color: var(--text-primary);
  }

  .list-row.selected {
    background: rgba(255, 255, 255, 0.04);
  }

  /* Column widths */
  .col-name { flex: 2; display: flex; align-items: center; gap: 8px; font-weight: 600; color: var(--text-primary); }
  .col-host { flex: 2.5; }
  .col-tags { flex: 2; display: flex; gap: 4px; flex-wrap: wrap; }
  .col-last { flex: 1.5; }
  .col-actions { flex: 2; display: flex; justify-content: flex-end; gap: 4px; opacity: 0; transition: opacity 0.2s; }

  .list-row:hover .col-actions, .list-row.selected .col-actions {
    opacity: 1;
  }

  .row-icon {
    color: var(--text-muted);
  }

  .row-badge {
    font-size: 0.7rem;
    padding: 1px 5px;
    border-radius: 4px;
    font-weight: 600;
  }

  .row-badge.krb {
    background: rgba(0, 240, 255, 0.08);
    border: 1px solid rgba(0, 240, 255, 0.2);
    color: var(--accent-cyan);
  }

  .tag-pill {
    font-size: 0.7rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    padding: 1px 6px;
    border-radius: 4px;
  }

  .row-action-btn {
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-secondary);
    padding: 4px;
    border-radius: 4px;
    cursor: pointer;
    display: flex;
  }

  .row-action-btn:hover {
    background: rgba(255, 255, 255, 0.06);
    color: var(--text-primary);
  }

  .row-action-btn.connect {
    background: rgba(0, 240, 255, 0.1);
    color: var(--accent-cyan);
  }

  .row-action-btn.connect:hover {
    background: var(--accent-cyan);
    color: var(--bg-app);
  }

  /* --------------------------------- */
  /* PREMIUM CARDS GRID VIEW           */
  /* --------------------------------- */
  .hosts-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 1rem;
  }

  .host-card {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    position: relative;
    overflow: hidden;
    transition: all 0.2s;
    cursor: pointer;
    display: flex;
    flex-direction: column;
  }

  .host-card:hover, .host-card.selected {
    border-color: var(--border-color-hover);
    box-shadow: 0 4px 20px rgba(0,0,0,0.3);
  }

  .host-card.selected {
    border-color: rgba(255,255,255,0.25);
  }

  .card-left-accent {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 3px;
    background: var(--text-muted);
  }

  .host-card:hover .card-left-accent, .host-card.selected .card-left-accent {
    background: var(--accent-cyan);
  }

  .card-content {
    padding: 1rem;
    display: flex;
    flex-direction: column;
    height: 100%;
    box-sizing: border-box;
  }

  .card-top {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 0.75rem;
  }

  .card-title-block h4 {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 700;
  }

  .host-addr {
    font-size: 0.75rem;
    color: var(--text-muted);
    margin-top: 2px;
    display: block;
  }

  .card-krb-badge {
    font-size: 0.65rem;
    background: rgba(0, 240, 255, 0.05);
    border: 1px solid rgba(0, 240, 255, 0.15);
    color: var(--accent-cyan);
    padding: 1px 4px;
    border-radius: 4px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .card-tags-row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 1.25rem;
  }

  .card-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-top: 1px solid var(--border-color);
    padding-top: 0.75rem;
    margin-top: auto;
  }

  .card-footer .last-used {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .card-btns {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .card-btn-icon {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 3px;
    border-radius: 4px;
  }

  .card-btn-icon:hover {
    color: var(--text-primary);
    background: rgba(255,255,255,0.06);
  }

  .card-connect-btn-small {
    background: var(--text-primary);
    border: none;
    color: var(--bg-app);
    font-size: 0.75rem;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 4px;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .card-connect-btn-small:hover {
    opacity: 0.9;
  }

  /* TERMINALS */
  .terminals-view {
    height: calc(100vh - 96px);
    display: flex;
    flex-direction: column;
  }

  .terminal-tabs {
    display: flex;
    gap: 2px;
    padding-bottom: 2px;
    border-bottom: 1px solid var(--border-color);
  }

  .terminal-tab-btn {
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-secondary);
    padding: 0.5rem 0.85rem;
    border-radius: 6px 6px 0 0;
    cursor: pointer;
    font-size: 0.8rem;
    display: flex;
    align-items: center;
    gap: 8px;
    transition: all 0.2s;
  }

  .terminal-tab-btn:hover {
    background: rgba(255,255,255,0.03);
    color: var(--text-primary);
  }

  .terminal-tab-btn.active {
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-primary);
    border: 1px solid var(--border-color);
    border-bottom-color: transparent;
  }

  .tab-close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 1px;
    border-radius: 50%;
    display: flex;
  }

  .tab-close-btn:hover {
    background: rgba(255, 255, 255, 0.1);
    color: var(--red-rose);
  }

  .terminal-viewport-container {
    flex: 1;
    background: #0c0d12;
    border: 1px solid var(--border-color);
    border-top: none;
    border-radius: 0 0 6px 6px;
    overflow: hidden;
  }

  .terminal-viewport {
    height: 100%;
    width: 100%;
    padding: 12px;
    box-sizing: border-box;
  }

  .terminal-viewport.hidden {
    display: none !important;
  }

  /* LOGS TABLE */
  .table-container {
    border: 1px solid var(--border-color);
    border-radius: 8px;
    overflow: hidden;
  }

  .history-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .history-table th {
    text-align: left;
    color: var(--text-muted);
    font-weight: 600;
    padding: 0.75rem 1rem;
    background: rgba(255, 255, 255, 0.02);
    border-bottom: 1px solid var(--border-color);
    font-size: 0.75rem;
    text-transform: uppercase;
  }

  .history-table td {
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border-color);
  }

  .history-table tr:last-child td {
    border-bottom: none;
  }

  .status-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 0.75rem;
    font-weight: 600;
  }

  .status-badge.success { color: var(--green-emerald); }
  .status-badge.error { color: var(--red-rose); }

  /* MODALS (SLIDE-OVER / DRAWER) */
  .modal-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal-dialog {
    background: var(--bg-app);
    border: 1px solid var(--border-color);
    border-radius: 12px;
    width: 520px;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    animation: slideUp 0.25s cubic-bezier(0.16, 1, 0.3, 1) forwards;
  }

  .modal-dialog.mini {
    width: 380px;
  }

  @keyframes slideUp {
    from { transform: translateY(15px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.25rem 1.5rem;
    border-bottom: 1px solid var(--border-color);
  }

  .modal-header h2 {
    font-size: 1.1rem;
    margin: 0;
    font-weight: 700;
    letter-spacing: -0.3px;
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    padding: 4px;
    border-radius: 4px;
  }

  .close-btn:hover {
    color: var(--text-primary);
    background: rgba(255,255,255,0.05);
  }

  .modal-body {
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    max-height: 60vh;
    overflow-y: auto;
  }

  .form-row {
    display: flex;
    gap: 1rem;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }

  .form-group label {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .cyber-input {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    padding: 0.6rem 0.75rem;
    border-radius: 6px;
    outline: none;
    font-size: 0.9rem;
  }

  .cyber-input:focus {
    border-color: rgba(255,255,255,0.25);
  }

  .input-with-action {
    display: flex;
    gap: 6px;
  }

  .input-with-action .cyber-input { flex: 1; }

  .browse-btn {
    background: rgba(255,255,255,0.04);
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    padding: 0 0.85rem;
    border-radius: 6px;
    cursor: pointer;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8rem;
  }

  .browse-btn:hover {
    border-color: var(--border-color-hover);
    color: var(--text-primary);
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.85rem;
    cursor: pointer;
    user-select: none;
  }

  .checkbox-row input { cursor: pointer; }

  .form-section-divider {
    font-size: 0.65rem;
    font-weight: 800;
    letter-spacing: 1px;
    color: var(--accent-cyan);
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0.25rem 0;
  }

  .form-section-divider::after {
    content: '';
    flex: 1;
    height: 1px;
    background: rgba(0, 240, 255, 0.15);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 1.25rem 1.5rem;
    border-top: 1px solid var(--border-color);
  }

  .cancel-btn, .save-btn {
    padding: 0.5rem 1rem;
    border-radius: 6px;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
  }

  .cancel-btn {
    background: transparent;
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
  }

  .cancel-btn:hover {
    border-color: var(--border-color-hover);
    color: var(--text-primary);
  }

  .save-btn {
    background: var(--text-primary);
    border: none;
    color: var(--bg-app);
  }

  .save-btn:hover {
    opacity: 0.9;
  }

  .profile-manager-list {
    margin-top: 1rem;
    border-top: 1px solid var(--border-color);
    padding-top: 1rem;
  }

  .profile-rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 6px;
  }

  .profile-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    background: rgba(255,255,255,0.02);
    border: 1px solid var(--border-color);
    padding: 0.4rem 0.75rem;
    border-radius: 6px;
    font-size: 0.85rem;
  }

  .profile-name.active {
    color: var(--accent-cyan);
    font-weight: 600;
  }

  .profile-delete-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    display: flex;
  }

  .profile-delete-btn:hover {
    color: var(--red-rose);
  }

  /* TOAST NOTIFICATION */
  .toast-notification {
    position: fixed;
    bottom: 20px;
    right: 20px;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    padding: 0.75rem 1.25rem;
    border-radius: 6px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    z-index: 1000;
    font-size: 0.85rem;
    display: flex;
    align-items: center;
    gap: 8px;
    animation: slideInLeft 0.25s cubic-bezier(0.16, 1, 0.3, 1) forwards;
  }

  .toast-notification.success { border-color: rgba(16, 185, 129, 0.3); }
  .toast-notification.error { border-color: rgba(244, 63, 94, 0.3); }

  @keyframes slideInLeft {
    from { transform: translateX(100%); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 5rem 2rem;
    text-align: center;
    color: var(--text-secondary);
    gap: 12px;
  }

  .empty-icon {
    color: var(--text-muted);
  }

  .empty-state h3 {
    margin: 0;
    font-size: 1.1rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .empty-state p {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text-muted);
    max-width: 280px;
    line-height: 1.4;
    margin-bottom: 0.5rem;
  }

  /* RESPONSIVENESS AND COLLAPSED MODE */
  @media (max-width: 900px) {
    .sidebar {
      width: 68px;
      padding: 1.5rem 0.5rem;
    }
    .logo-text, .section-title, .nav-label, .sidebar-stats, .quick-tags, .sidebar-toggle-btn {
      display: none !important;
    }
    .logo-container, .nav-item {
      justify-content: center !important;
      padding-left: 0 !important;
      padding-right: 0 !important;
    }
    .badge {
      display: none;
    }
  }
</style>
