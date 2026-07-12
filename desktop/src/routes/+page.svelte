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
  import OnboardingModal from "$lib/components/modals/OnboardingModal.svelte";
  import Toast from "$lib/components/Toast.svelte";

  import type {
    AppTab,
    Connection,
    ConnectionStats,
    DesktopSettings,
    EnvInfo,
    OnboardingPayload,
    SessionHistoryEntry,
    WorkspaceInfo,
  } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { applyTheme } from "$lib/utils/theme";
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
  let showOnboarding = $state(false);

  let workspace = $state<WorkspaceInfo>({
    active_env: "default",
    config_root: "",
    env_dir: "",
    config_path: "",
    database_path: "",
    ssh_config_path: "",
    default_user: "root",
    default_port: 22,
    search_mode: "bayesian",
  });

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
      await loadWorkspace();
      await loadConnections();
      await loadStats();
      await loadHistory();
      await loadSettings();
      await loadAgentStatus();
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  async function loadWorkspace() {
    try {
      workspace = await invoke("get_workspace_info");
      activeEnv = workspace.active_env;
    } catch (e) {
      console.error("Failed to load workspace info", e);
    }
  }

  async function checkOnboarding() {
    try {
      const needsSetup = await invoke<boolean>("needs_onboarding");
      showOnboarding = needsSetup;
    } catch (e) {
      console.error("Failed to check onboarding state", e);
    }
  }

  async function saveWorkspaceConfig() {
    try {
      await invoke("save_workspace_config", {
        update: {
          default_user: settings.default_user,
          default_port: settings.default_port,
          ssh_config_path: workspace.ssh_config_path || "",
          search_mode: settings.fuzzy_search ? "fuzzy" : "bayesian",
        },
      });
      await loadWorkspace();
      notify("Workspace settings saved", "success");
    } catch (e: unknown) {
      notify(`Failed to save workspace: ${e}`, "error");
    }
  }

  async function browseSshConfig(): Promise<string | null> {
    try {
      const selected = await invoke<string | null>("pick_ssh_config_file");
      if (selected) {
        workspace = { ...workspace, ssh_config_path: selected };
        if (!showOnboarding) {
          await saveWorkspaceConfig();
        }
      }
      return selected;
    } catch (e: unknown) {
      notify(String(e), "error");
      return null;
    }
  }

  async function importSshConfig() {
    try {
      const count = await invoke<number>("import_ssh_config", {
        file: workspace.ssh_config_path || null,
      });
      await loadConnections();
      await loadStats();
      notify(
        count > 0 ? `Imported ${count} host${count === 1 ? "" : "s"} from OpenSSH config` : "No new hosts to import",
        count > 0 ? "success" : "info",
      );
    } catch (e: unknown) {
      notify(`Import failed: ${e}`, "error");
    }
  }

  async function completeOnboarding(payload: OnboardingPayload) {
    try {
      const imported = await invoke<number>("complete_onboarding", { payload });
      showOnboarding = false;
      await loadData();
      applyTheme(settings.theme);
      if (imported > 0) {
        notify(`Setup complete — imported ${imported} host${imported === 1 ? "" : "s"}`, "success");
      } else {
        notify("Workspace ready. Add your first host whenever you like.", "success");
      }
    } catch (e: unknown) {
      notify(`Setup failed: ${e}`, "error");
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
    applyTheme(settings.theme);
    try {
      await invoke("save_desktop_settings", {
        settings: { ...settings, onboarding_complete: true },
      });
      notify("Settings saved successfully", "success");
    } catch (e: unknown) {
      notify(`Failed to save settings: ${e}`, "error");
    }
  }

  function handleThemeChange(theme: string) {
    settings.theme = theme;
    applyTheme(theme);
    void saveSettings();
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

  /** Reload list after add/edit/duplicate — clears stale search/tag filters that hide results. */
  async function reloadConnectionsAfterMutation() {
    try {
      const allConnections = await invoke<Connection[]>("get_connections", {
        query: "",
        tagFilter: null,
      });

      if (searchQuery.trim() || selectedTag) {
        const filtered = await invoke<Connection[]>("get_connections", {
          query: searchQuery,
          tagFilter: selectedTag,
        });

        if (filtered.length === 0 && allConnections.length > 0) {
          searchQuery = "";
          selectedTag = null;
          connections = allConnections;
          notify("Search cleared — the saved host no longer matches your filter", "info");
          return;
        }

        connections = filtered;
        return;
      }

      connections = allConnections;
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

      await reloadConnectionsAfterMutation();
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
      await reloadConnectionsAfterMutation();
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
    try {
      await connectSSH(conn);
      requestAnimationFrame(() => fitActiveTerminal());
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  function handleTabChange(tab: AppTab) {
    activeTab = tab;
    if (tab === "terminals") {
      requestAnimationFrame(() => fitActiveTerminal());
    }
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    if (showOnboarding || showModal || showEnvModal) {
      if (e.key === "Escape" && !showOnboarding) {
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

  const allTags = $derived.by(() => {
    const tagsSet = new Set<string>();
    connections.forEach((c) => c.tags.forEach((t) => tagsSet.add(t)));
    return Array.from(tagsSet).sort();
  });

  const showTerminalsPanel = $derived(activeTab === "terminals" || terminalState.count > 0);

  onMount(() => {
    (async () => {
      await loadData();
      await checkOnboarding();
    })();
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
            <SettingsView
              bind:settings
              bind:workspace
              {environments}
              onSave={saveSettings}
              onThemeChange={handleThemeChange}
              onSaveWorkspace={saveWorkspaceConfig}
              onSwitchEnv={switchEnv}
              onManageProfiles={() => (showEnvModal = true)}
              onBrowseSshConfig={browseSshConfig}
              onImportSshConfig={importSshConfig}
            />
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

  {#if showOnboarding}
    <OnboardingModal
      defaultUser={settings.default_user}
      defaultSshConfigPath={workspace.ssh_config_path || ""}
      configRoot={workspace.config_root}
      onBrowseSshConfig={browseSshConfig}
      onComplete={completeOnboarding}
    />
  {/if}

  <Toast />
</div>
