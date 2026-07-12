<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Plus, Search, List, LayoutGrid } from "lucide-svelte";

  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ConnectionsView from "$lib/components/ConnectionsView.svelte";
  import TerminalsView from "$lib/components/TerminalsView.svelte";
  import HistoryView from "$lib/components/HistoryView.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import ConnectionModal from "$lib/components/modals/ConnectionModal.svelte";
  import EnvModal from "$lib/components/modals/EnvModal.svelte";
  import AgentModal from "$lib/components/modals/AgentModal.svelte";
  import DeleteConfirm from "$lib/components/modals/DeleteConfirm.svelte";
  import Toast from "$lib/components/Toast.svelte";

  import type {
    AppTab,
    Connection,
    ConnectionStats,
    DesktopSettings,
    EnvInfo,
    SessionHistoryEntry,
  } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import {
    connectSSH,
    fitActiveTerminal,
    getTerminalState,
    initTerminalListeners,
    teardownTerminalListeners,
  } from "$lib/stores/terminal.svelte";
  import { getWindowState, initWindowState } from "$lib/stores/window.svelte";

  const terminalState = getTerminalState();
  const windowState = getWindowState();

  let activeTab = $state<AppTab>("connections");
  let environments = $state<EnvInfo[]>([]);
  let activeEnv = $state("default");
  let connections = $state<Connection[]>([]);
  let searchQuery = $state("");
  let selectedTag = $state<string | null>(null);
  let stats = $state<ConnectionStats | null>(null);
  let history = $state<SessionHistoryEntry[]>([]);

  let viewMode = $state<"list" | "grid">("list");
  let sidebarCollapsed = $state(false);
  let selectedHostIndex = $state(0);

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

  let showEnvModal = $state(false);
  let newEnvName = $state("");

  let copiedId = $state<string | null>(null);
  let justDuplicatedId = $state<string | null>(null);

  let showDeleteConfirm = $state(false);
  let deleteTarget = $state<{
    label: string;
    subtitle: string;
    onConfirm: () => Promise<void>;
  } | null>(null);

  let agentActive = $state(false);
  let agentSocket = $state<string | null>(null);
  let agentKeys = $state<string[]>([]);
  let showAgentModal = $state(false);

  let settings = $state<DesktopSettings>({
    theme: "zinc",
    auto_start_agent: false,
    custom_agent_socket: "",
    default_user: "root",
    default_port: 22,
    fuzzy_search: false,
    default_key_path: "",
  });

  function promptDelete(label: string, subtitle: string, onConfirm: () => Promise<void>) {
    deleteTarget = { label, subtitle, onConfirm };
    showDeleteConfirm = true;
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    showDeleteConfirm = false;
    try {
      await deleteTarget.onConfirm();
    } finally {
      deleteTarget = null;
    }
  }

  async function loadData() {
    try {
      activeEnv = await invoke("get_active_env");
      environments = await invoke("list_environments");
      await loadConnections();
      await loadStats();
      await loadHistory();
      await loadSettings();
      await loadAgentStatus();
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  async function loadSettings() {
    try {
      const loaded: Record<string, unknown> = await invoke("load_desktop_settings");
      settings = {
        theme: (loaded.theme as string) || "zinc",
        auto_start_agent: Boolean(loaded.auto_start_agent),
        custom_agent_socket: (loaded.custom_agent_socket as string) || "",
        default_user: (loaded.default_user as string) || "root",
        default_port: (loaded.default_port as number) || 22,
        fuzzy_search: Boolean(loaded.fuzzy_search),
        default_key_path: (loaded.default_key_path as string) || "",
      };
      applyTheme(settings.theme);

      if (settings.auto_start_agent && !agentActive) {
        await triggerStartAgent();
      }
    } catch (e) {
      console.error("Failed to load settings", e);
    }
  }

  async function saveSettings() {
    try {
      await invoke("save_desktop_settings", { settings });
      applyTheme(settings.theme);
      notify("Settings saved successfully", "success");
    } catch (e: unknown) {
      notify(`Failed to save settings: ${e}`, "error");
    }
  }

  function applyTheme(themeName: string) {
    document.documentElement.classList.remove("theme-zinc", "theme-cyberpunk", "theme-oled", "theme-slate");
    document.documentElement.classList.add(`theme-${themeName}`);
  }

  async function loadAgentStatus() {
    try {
      const status: { active: boolean; socket_path: string | null; keys: string[] } =
        await invoke("get_agent_status");
      agentActive = status.active;
      agentSocket = status.socket_path;
      agentKeys = status.keys;
    } catch (e) {
      console.error("Failed to load agent status", e);
    }
  }

  async function triggerStartAgent() {
    try {
      const status: { active: boolean; socket_path: string | null; keys: string[] } =
        await invoke("start_agent");
      agentActive = status.active;
      agentSocket = status.socket_path;
      agentKeys = status.keys;
      notify("SSH Agent started successfully", "success");
    } catch (e: unknown) {
      notify(`Failed to start agent: ${e}`, "error");
    }
  }

  async function triggerAddKey(keyPath: string) {
    try {
      await invoke("add_key_to_agent", { keyPath });
      await loadAgentStatus();
      notify("Key added to SSH Agent successfully", "success");
    } catch (e: unknown) {
      notify(`Failed to add key: ${e}`, "error");
    }
  }

  async function selectAndAddKey() {
    try {
      const file = await invoke<string | null>("pick_key_file");
      if (file) await triggerAddKey(file);
    } catch (e) {
      console.error("Failed to pick key file", e);
    }
  }

  async function loadConnections() {
    try {
      connections = await invoke("get_connections", {
        query: searchQuery,
        tagFilter: selectedTag,
      });
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  async function loadStats() {
    try {
      stats = await invoke("get_stats");
    } catch {
      // optional
    }
  }

  async function loadHistory() {
    try {
      history = await invoke("get_history", { limit: 50 });
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  async function duplicateConnection(conn: Connection) {
    try {
      const copyName = `${conn.name} (Copy)`;
      await invoke("add_connection", {
        name: copyName,
        host: conn.host,
        user: conn.user,
        port: conn.port,
        kerberos: conn.use_kerberos,
        bastion: conn.bastion || null,
        bastionUser: conn.bastion_user || null,
        keyPath: conn.key_path || null,
        tags: [...conn.tags],
      });

      await loadConnections();
      await loadStats();

      const newIdx = connections.findIndex((c) => c.name === copyName && c.host === conn.host);
      if (newIdx !== -1) {
        selectedHostIndex = newIdx;
        justDuplicatedId = connections[newIdx].id;
        setTimeout(() => {
          justDuplicatedId = null;
        }, 2000);
        openEditModal(connections[newIdx]);
      }

      notify("Connection duplicated — update values below", "info");
    } catch (e: unknown) {
      notify(`Failed to duplicate: ${e}`, "error");
    }
  }

  async function copyToClipboard(text: string, id: string) {
    try {
      await navigator.clipboard.writeText(text);
      copiedId = id;
      setTimeout(() => {
        if (copiedId === id) copiedId = null;
      }, 1500);
      notify("SSH command copied to clipboard", "success");
    } catch {
      notify("Failed to copy", "error");
    }
  }

  async function switchEnv(envName: string) {
    try {
      await invoke("set_active_env", { name: envName });
      await loadData();
      notify(`Switched to profile '${envName}'`, "success");
    } catch (e: unknown) {
      notify(String(e), "error");
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
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  async function deleteEnv(envName: string) {
    promptDelete(envName, "All hosts in this profile will be permanently removed.", async () => {
      await invoke("remove_environment", { name: envName });
      await loadData();
      notify(`Profile '${envName}' deleted`, "success");
    });
  }

  async function browseKey() {
    try {
      const selected = await invoke<string | null>("pick_key_file");
      if (selected) modalKeyPath = selected;
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

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
      .map((t) => t.trim())
      .filter((t) => t.length > 0);

    const payload = {
      name: modalName.trim(),
      host: modalHost.trim(),
      user: modalUser.trim() || null,
      port: modalPort || null,
      kerberos: modalUseKerberos || null,
      bastion: modalBastion.trim() || null,
      bastionUser: modalBastionUser.trim() || null,
      keyPath: modalKeyPath.trim() || null,
      tags,
    };

    try {
      if (isEditing) {
        await invoke("edit_connection", {
          id: modalConnectionId,
          ...payload,
          user: modalUser,
          port: modalPort,
          kerberos: modalUseKerberos,
        });
        notify("Host updated successfully", "success");
      } else {
        await invoke("add_connection", payload);
        notify("Host added successfully", "success");
      }
      showModal = false;
      await loadConnections();
      await loadStats();
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  async function deleteConnection(conn: Connection) {
    promptDelete(conn.name, `${conn.user}@${conn.host}:${conn.port}`, async () => {
      await invoke("remove_connection", { idOrName: conn.id });
      notify(`'${conn.name}' removed`, "success");
      await loadConnections();
      await loadStats();
    });
  }

  async function handleConnect(conn: Connection) {
    activeTab = "terminals";
    await tick();
    await connectSSH(conn);
    requestAnimationFrame(() => fitActiveTerminal());
  }

  function handleTabChange(tab: AppTab) {
    activeTab = tab;
    if (tab === "terminals") {
      requestAnimationFrame(() => fitActiveTerminal());
    }
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    if (showModal || showEnvModal) {
      if (e.key === "Escape") {
        showModal = false;
        showEnvModal = false;
      }
      return;
    }

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
        if (connections[selectedHostIndex]) handleConnect(connections[selectedHostIndex]);
      } else if (e.key === "e" && e.ctrlKey) {
        e.preventDefault();
        if (connections[selectedHostIndex]) openEditModal(connections[selectedHostIndex]);
      }
    }
  }

  $effect(() => {
    if (connections) selectedHostIndex = 0;
  });

  $effect(() => {
    searchQuery;
    selectedTag;
    loadConnections();
  });

  $effect(() => {
    if (activeTab === "terminals") {
      requestAnimationFrame(() => fitActiveTerminal());
    }
  });

  $effect(() => {
    if (terminalState.count === 0 && activeTab === "terminals") {
      activeTab = "connections";
    }
  });

  const allTags = $derived.by(() => {
    const tagsSet = new Set<string>();
    connections.forEach((c) => c.tags.forEach((t) => tagsSet.add(t)));
    return Array.from(tagsSet).sort();
  });

  const showTerminalsPanel = $derived(activeTab === "terminals" || terminalState.count > 0);

  onMount(() => {
    loadData();
    window.addEventListener("keydown", handleGlobalKeydown);

    initTerminalListeners(async () => {
      await loadHistory();
      await loadStats();
    });

    let teardownWindow = () => {};
    initWindowState().then((teardown) => {
      teardownWindow = teardown;
    });

    return () => {
      window.removeEventListener("keydown", handleGlobalKeydown);
      teardownTerminalListeners();
      teardownWindow();
    };
  });
</script>

<div class="window-container" class:is-fullscreen={windowState.isFullscreen}>
  <TitleBar {activeEnv} />

  <div class="app-layout" class:collapsed={sidebarCollapsed}>
    <Sidebar
      {activeTab}
      onTabChange={handleTabChange}
      {environments}
      {activeEnv}
      onSwitchEnv={switchEnv}
      onShowEnvModal={() => (showEnvModal = true)}
      {stats}
      {sidebarCollapsed}
      onToggleSidebar={() => (sidebarCollapsed = !sidebarCollapsed)}
      terminalCount={terminalState.count}
      {allTags}
      {selectedTag}
      onTagSelect={(tag) => {
        selectedTag = tag;
        loadConnections();
      }}
      {agentActive}
      {agentKeys}
      onStartAgent={triggerStartAgent}
      onShowAgentModal={() => (showAgentModal = true)}
      onSearchMostUsed={(name) => (searchQuery = name)}
    />

    <main class="main-panel">
      <header class="topbar">
        <div class="search-section">
          {#if activeTab !== "terminals"}
            <div class="search-container">
              <Search class="search-icon" size={16} />
              <input
                type="text"
                placeholder="Search host, alias, or tag... (Press '/' to focus)"
                bind:value={searchQuery}
                class="search-input"
              />
            </div>

            {#if activeTab === "connections"}
              <div class="view-mode-toggle">
                <button
                  class="toggle-btn"
                  class:active={viewMode === "list"}
                  onclick={() => (viewMode = "list")}
                  title="List View"
                >
                  <List size={16} />
                </button>
                <button
                  class="toggle-btn"
                  class:active={viewMode === "grid"}
                  onclick={() => (viewMode = "grid")}
                  title="Grid View"
                >
                  <LayoutGrid size={16} />
                </button>
              </div>
            {/if}
          {:else}
            <span class="topbar-context-label">Active SSH Sessions</span>
          {/if}
        </div>

        <div class="topbar-actions">
          <button class="cyber-btn" onclick={openAddModal}>
            <Plus size={16} />
            <span>New Server</span>
          </button>
        </div>
      </header>

      <div class="main-body">
        {#if activeTab === "connections"}
          <div class="main-body-panel is-visible">
            <ConnectionsView
              {connections}
              {viewMode}
              {selectedHostIndex}
              {copiedId}
              {justDuplicatedId}
              onSelectHost={(i) => (selectedHostIndex = i)}
              onConnect={handleConnect}
              onEdit={openEditModal}
              onDelete={deleteConnection}
              onDuplicate={duplicateConnection}
              onCopyCommand={copyToClipboard}
              onRefresh={loadConnections}
              onAddHost={openAddModal}
            />
          </div>
        {/if}

        {#if activeTab === "history"}
          <div class="main-body-panel is-visible">
            <HistoryView {history} />
          </div>
        {/if}

        {#if activeTab === "settings"}
          <div class="main-body-panel is-visible">
            <SettingsView bind:settings onSave={saveSettings} />
          </div>
        {/if}

        {#if showTerminalsPanel}
          <div
            class="main-body-panel"
            class:is-visible={activeTab === "terminals"}
            class:is-docked={activeTab !== "terminals"}
          >
            <TerminalsView {connections} bind:searchQuery onSearchInput={loadConnections} />
          </div>
        {/if}
      </div>
    </main>
  </div>

  {#if showModal}
    <ConnectionModal
      {isEditing}
      bind:modalName
      bind:modalHost
      bind:modalUser
      bind:modalPort
      bind:modalUseKerberos
      bind:modalBastion
      bind:modalBastionUser
      bind:modalKeyPath
      bind:modalTagsString
      onClose={() => (showModal = false)}
      onSave={saveConnection}
      onBrowseKey={browseKey}
    />
  {/if}

  {#if showEnvModal}
    <EnvModal
      {environments}
      bind:newEnvName
      onClose={() => (showEnvModal = false)}
      onCreate={createEnv}
      onDelete={deleteEnv}
    />
  {/if}

  {#if showAgentModal}
    <AgentModal
      {agentSocket}
      {agentKeys}
      onClose={() => (showAgentModal = false)}
      onAddKey={selectAndAddKey}
    />
  {/if}

  {#if showDeleteConfirm && deleteTarget}
    <DeleteConfirm
      label={deleteTarget.label}
      subtitle={deleteTarget.subtitle}
      onCancel={() => {
        showDeleteConfirm = false;
        deleteTarget = null;
      }}
      onConfirm={confirmDelete}
    />
  {/if}

  <Toast />
</div>
