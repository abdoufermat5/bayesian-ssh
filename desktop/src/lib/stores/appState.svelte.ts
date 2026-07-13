import { tick } from "svelte";
import { invoke } from "@tauri-apps/api/core";
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
  closeAllTabs,
  connectSSH,
  dockPopoutSession,
  fitActiveTerminal,
  focusPopoutSession,
  getTerminalState,
  initTerminalListeners,
  popOutDetachedSession,
  reattachSession,
  terminateAllDetachedSessions,
  terminateDetachedSession,
  terminatePopoutSession,
} from "$lib/stores/terminal.svelte";
import { getWindowState } from "$lib/stores/window.svelte";
import {
  acquireKerberosTicket,
  closeKerberosModal,
  consumePendingConnection,
  formatKerberosRemaining,
  getKerberosHealth,
  getKerberosState,
  getLiveRemainingSeconds,
  openKerberosModal,
  renewKerberosTicket,
  startKerberosMonitoring,
  stopKerberosMonitoring,
} from "$lib/stores/kerberos.svelte";

const terminalState = getTerminalState();
const windowState = getWindowState();
const kerberosState = getKerberosState();

export class AppStateStore {
  activeTab = $state<AppTab>("connections");
  environments = $state<EnvInfo[]>([]);
  activeEnv = $state("default");
  connections = $state<Connection[]>([]);
  searchQuery = $state("");
  selectedTag = $state<string | null>(null);
  stats = $state<ConnectionStats | null>(null);
  history = $state<SessionHistoryEntry[]>([]);

  viewMode = $state<"list" | "grid">("list");
  sidebarCollapsed = $state(false);
  selectedHostIndex = $state(0);

  showModal = $state(false);
  isEditing = $state(false);
  modalConnectionId = $state("");
  modalName = $state("");
  modalHost = $state("");
  modalUser = $state("");
  modalPort = $state(22);
  modalUseKerberos = $state(false);
  modalBastion = $state("");
  modalBastionUser = $state("");
  modalKeyPath = $state("");
  modalTagsString = $state("");

  showEnvModal = $state(false);
  newEnvName = $state("");

  copiedId = $state<string | null>(null);
  justDuplicatedId = $state<string | null>(null);

  showDeleteConfirm = $state(false);
  deleteTarget = $state<{
    title?: string;
    confirmLabel?: string;
    warning?: string;
    label: string;
    subtitle: string;
    onConfirm: () => Promise<void>;
  } | null>(null);

  agentActive = $state(false);
  agentSocket = $state<string | null>(null);
  agentKeys = $state<string[]>([]);
  showAgentModal = $state(false);
  showSessionManager = $state(false);
  kerberosLoading = $state(false);
  kerberosError = $state<string | null>(null);
  showOnboarding = $state(false);

  workspace = $state<WorkspaceInfo>({
    active_env: "default",
    config_root: "",
    env_dir: "",
    config_path: "",
    database_path: "",
    ssh_config_path: "",
    default_user: "root",
    default_port: 22,
    search_mode: "bayesian",
    log_level: "info",
    auto_save_history: true,
    max_history_size: 1000,
  });

  settings = $state<DesktopSettings>({
    theme: "zinc",
    auto_start_agent: false,
    custom_agent_socket: "",
    kerberos_warn_minutes: 15,
    monitor_kerberos: true,
    default_user: "root",
    default_port: 22,
    fuzzy_search: false,
    default_key_path: "",
    timezone: "system",
  });

  allTags = $derived.by(() => {
    const tagsSet = new Set<string>();
    this.connections.forEach((c) => c.tags.forEach((t) => tagsSet.add(t)));
    return Array.from(tagsSet).sort();
  });

  kerberosHealth = $derived(
    getKerberosHealth(getLiveRemainingSeconds(), this.settings.kerberos_warn_minutes),
  );
  kerberosRemainingLabel = $derived(formatKerberosRemaining(getLiveRemainingSeconds()));

  showTerminalsPanel = $derived(
    this.activeTab === "terminals" ||
      terminalState.count > 0 ||
      terminalState.externalSessionCount > 0,
  );

  promptDelete(
    label: string,
    subtitle: string,
    onConfirm: () => Promise<void>,
    options?: { title?: string; confirmLabel?: string; warning?: string },
  ) {
    this.deleteTarget = { label, subtitle, onConfirm, ...options };
    this.showDeleteConfirm = true;
  }

  confirmDelete = async () => {
    if (!this.deleteTarget) return;
    this.showDeleteConfirm = false;
    try {
      await this.deleteTarget.onConfirm();
    } finally {
      this.deleteTarget = null;
    }
  }

  loadData = async () => {
    try {
      this.activeEnv = await invoke("get_active_env");
      this.environments = await invoke("list_environments");
      await this.loadWorkspace();
      await this.loadConnections();
      await this.loadStats();
      await this.loadHistory();
      await this.loadSettings();
      await this.loadAgentStatus();
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  loadWorkspace = async () => {
    try {
      this.workspace = await invoke("get_workspace_info");
      this.activeEnv = this.workspace.active_env;
    } catch (e) {
      console.error("Failed to load workspace info", e);
    }
  }

  checkOnboarding = async () => {
    try {
      const needsSetup = await invoke<boolean>("needs_onboarding");
      this.showOnboarding = needsSetup;
    } catch (e) {
      console.error("Failed to check onboarding state", e);
    }
  }

  saveWorkspaceConfig = async () => {
    try {
      await invoke("save_workspace_config", {
        update: {
          default_user: this.settings.default_user,
          default_port: this.settings.default_port,
          ssh_config_path: this.workspace.ssh_config_path || "",
          search_mode: this.settings.fuzzy_search ? "fuzzy" : "bayesian",
          log_level: this.workspace.log_level,
          auto_save_history: this.workspace.auto_save_history,
          max_history_size: this.workspace.max_history_size,
        },
      });
      await this.loadWorkspace();
      notify("Workspace settings saved", "success");
    } catch (e: unknown) {
      notify(`Failed to save workspace: ${e}`, "error");
    }
  }

  browseSshConfig = async (): Promise<string | null> => {
    try {
      const selected = await invoke<string | null>("pick_ssh_config_file");
      if (selected) {
        this.workspace = { ...this.workspace, ssh_config_path: selected };
        if (!this.showOnboarding) {
          await this.saveWorkspaceConfig();
        }
      }
      return selected;
    } catch (e: unknown) {
      notify(String(e), "error");
      return null;
    }
  }

  importSshConfig = async () => {
    try {
      const count = await invoke<number>("import_ssh_config", {
        file: this.workspace.ssh_config_path || null,
      });
      await this.loadConnections();
      await this.loadStats();
      notify(
        count > 0 ? `Imported ${count} host${count === 1 ? "" : "s"} from OpenSSH config` : "No new hosts to import",
        count > 0 ? "success" : "info",
      );
    } catch (e: unknown) {
      notify(`Import failed: ${e}`, "error");
    }
  }

  completeOnboarding = async (payload: OnboardingPayload) => {
    try {
      const imported = await invoke<number>("complete_onboarding", { payload });
      this.showOnboarding = false;
      await this.loadData();
      applyTheme(this.settings.theme);
      if (imported > 0) {
        notify(`Setup complete — imported ${imported} host${imported === 1 ? "" : "s"}`, "success");
      } else {
        notify("Workspace ready. Add your first host whenever you like.", "success");
      }
    } catch (e: unknown) {
      notify(`Setup failed: ${e}`, "error");
    }
  }

  loadSettings = async () => {
    try {
      const loaded: Record<string, unknown> = await invoke("load_desktop_settings");
      this.settings = {
        theme: (loaded.theme as string) || "zinc",
        auto_start_agent: Boolean(loaded.auto_start_agent),
        custom_agent_socket: (loaded.custom_agent_socket as string) || "",
        kerberos_warn_minutes: Number(loaded.kerberos_warn_minutes) || 15,
        monitor_kerberos: loaded.monitor_kerberos !== false,
        default_user: (loaded.default_user as string) || "root",
        default_port: (loaded.default_port as number) || 22,
        fuzzy_search: Boolean(loaded.fuzzy_search),
        default_key_path: (loaded.default_key_path as string) || "",
        timezone: (loaded.timezone as string) || "system",
      };
      applyTheme(this.settings.theme);

      if (this.settings.auto_start_agent && !this.agentActive) {
        await this.triggerStartAgent();
      }

      if (this.settings.monitor_kerberos) {
        startKerberosMonitoring({
          warnMinutes: this.settings.kerberos_warn_minutes,
          onWarning: (message) => notify(message, "info"),
        });
      } else {
        stopKerberosMonitoring();
      }
    } catch (e) {
      console.error("Failed to load settings", e);
    }
  }

  saveSettings = async () => {
    applyTheme(this.settings.theme);
    try {
      await invoke("save_desktop_settings", {
        settings: { ...this.settings, onboarding_complete: true },
      });
      if (this.settings.monitor_kerberos) {
        startKerberosMonitoring({
          warnMinutes: this.settings.kerberos_warn_minutes,
          onWarning: (message) => notify(message, "info"),
        });
      } else {
        stopKerberosMonitoring();
      }
      notify("Settings saved successfully", "success");
    } catch (e: unknown) {
      notify(`Failed to save settings: ${e}`, "error");
    }
  }

  handleThemeChange = (theme: string) => {
    this.settings.theme = theme;
    applyTheme(theme);
    void this.saveSettings();
  }

  loadAgentStatus = async () => {
    try {
      const status: { active: boolean; socket_path: string | null; keys: string[] } =
        await invoke("get_agent_status");
      this.agentActive = status.active;
      this.agentSocket = status.socket_path;
      this.agentKeys = status.keys;
    } catch (e) {
      console.error("Failed to load agent status", e);
    }
  }

  triggerStartAgent = async () => {
    try {
      const status: { active: boolean; socket_path: string | null; keys: string[] } =
        await invoke("start_agent");
      this.agentActive = status.active;
      this.agentSocket = status.socket_path;
      this.agentKeys = status.keys;
      notify("SSH Agent started successfully", "success");
    } catch (e: unknown) {
      notify(`Failed to start agent: ${e}`, "error");
    }
  }

  triggerAddKey = async (keyPath: string) => {
    try {
      await invoke("add_key_to_agent", { keyPath });
      await this.loadAgentStatus();
      notify("Key added to SSH Agent successfully", "success");
    } catch (e: unknown) {
      notify(`Failed to add key: ${e}`, "error");
    }
  }

  selectAndAddKey = async () => {
    try {
      const file = await invoke<string | null>("pick_key_file");
      if (file) await this.triggerAddKey(file);
    } catch (e) {
      console.error("Failed to pick key file", e);
    }
  }

  resumePendingKerberosConnection = async () => {
    const conn = consumePendingConnection();
    if (!conn) return;
    this.activeTab = "terminals";
    await tick();
    await connectSSH(conn);
    requestAnimationFrame(() => fitActiveTerminal());
  }

  handleKerberosRenew = async (password?: string) => {
    this.kerberosLoading = true;
    this.kerberosError = null;
    try {
      const next = await renewKerberosTicket(password);
      if (next.valid) {
        notify("Kerberos ticket renewed", "success");
        await this.resumePendingKerberosConnection();
        closeKerberosModal();
      }
    } catch (e: unknown) {
      this.kerberosError = String(e);
    } finally {
      this.kerberosLoading = false;
    }
  }

  handleKerberosAcquire = async (principal: string, password: string) => {
    this.kerberosLoading = true;
    this.kerberosError = null;
    try {
      const next = await acquireKerberosTicket(password, principal);
      if (next.valid) {
        notify("Kerberos ticket acquired", "success");
        await this.resumePendingKerberosConnection();
        closeKerberosModal();
      }
    } catch (e: unknown) {
      this.kerberosError = String(e);
    } finally {
      this.kerberosLoading = false;
    }
  }

  loadConnections = async () => {
    try {
      this.connections = await invoke("get_connections", {
        query: this.searchQuery,
        tagFilter: this.selectedTag,
      });
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  reloadConnectionsAfterMutation = async () => {
    try {
      const allConnections = await invoke<Connection[]>("get_connections", {
        query: "",
        tagFilter: null,
      });

      if (this.searchQuery.trim() || this.selectedTag) {
        const filtered = await invoke<Connection[]>("get_connections", {
          query: this.searchQuery,
          tagFilter: this.selectedTag,
        });

        if (filtered.length === 0 && allConnections.length > 0) {
          this.searchQuery = "";
          this.selectedTag = null;
          this.connections = allConnections;
          notify("Search cleared — the saved host no longer matches your filter", "info");
          return;
        }

        this.connections = filtered;
        return;
      }

      this.connections = allConnections;
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  loadStats = async () => {
    try {
      this.stats = await invoke("get_stats");
    } catch {
      // optional
    }
  }

  loadHistory = async () => {
    try {
      this.history = await invoke("get_history", { limit: 50 });
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  duplicateConnection = async (conn: Connection) => {
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

      await this.reloadConnectionsAfterMutation();
      await this.loadStats();

      const newIdx = this.connections.findIndex((c) => c.name === copyName && c.host === conn.host);
      if (newIdx !== -1) {
        this.selectedHostIndex = newIdx;
        this.justDuplicatedId = this.connections[newIdx].id;
        setTimeout(() => {
          this.justDuplicatedId = null;
        }, 2000);
        this.openEditModal(this.connections[newIdx]);
      }

      notify("Connection duplicated — update values below", "info");
    } catch (e: unknown) {
      notify(`Failed to duplicate: ${e}`, "error");
    }
  }

  copyToClipboard = async (text: string, id: string) => {
    try {
      await navigator.clipboard.writeText(text);
      this.copiedId = id;
      setTimeout(() => {
        if (this.copiedId === id) this.copiedId = null;
      }, 1500);
      notify("SSH command copied to clipboard", "success");
    } catch {
      notify("Failed to copy", "error");
    }
  }

  switchEnv = async (envName: string) => {
    try {
      await invoke("set_active_env", { name: envName });
      await this.loadData();
      notify(`Switched to profile '${envName}'`, "success");
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  createEnv = async () => {
    if (!this.newEnvName.trim()) return;
    try {
      await invoke("create_environment", { name: this.newEnvName.trim() });
      this.newEnvName = "";
      this.showEnvModal = false;
      await this.loadData();
      notify("Profile created successfully", "success");
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  deleteEnv = async (envName: string) => {
    this.promptDelete(envName, "All hosts in this profile will be permanently removed.", async () => {
      await invoke("remove_environment", { name: envName });
      await this.loadData();
      notify(`Profile '${envName}' deleted`, "success");
    });
  }

  browseKey = async () => {
    try {
      const selected = await invoke<string | null>("pick_key_file");
      if (selected) this.modalKeyPath = selected;
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  openSessionManager = () => {
    this.showSessionManager = true;
  }

  goToTerminals = () => {
    this.activeTab = "terminals";
    requestAnimationFrame(() => fitActiveTerminal());
  }

  handleSessionReattach = async (sessionId: string) => {
    await reattachSession(sessionId);
    this.showSessionManager = false;
    this.goToTerminals();
  }

  handleSessionDock = async (sessionId: string) => {
    await dockPopoutSession(sessionId);
    this.showSessionManager = false;
    this.goToTerminals();
  }

  handleTerminateAllSessions = async () => {
    await terminateAllDetachedSessions();
    for (const session of [...terminalState.popoutSessions]) {
      await terminatePopoutSession(session.id);
    }
  }

  openAddModal = () => {
    this.isEditing = false;
    this.modalConnectionId = "";
    this.modalName = "";
    this.modalHost = "";
    this.modalUser = "";
    this.modalPort = 22;
    this.modalUseKerberos = false;
    this.modalBastion = "";
    this.modalBastionUser = "";
    this.modalKeyPath = "";
    this.modalTagsString = "";
    this.showModal = true;
  }

  openEditModal = (conn: Connection) => {
    this.isEditing = true;
    this.modalConnectionId = conn.id;
    this.modalName = conn.name;
    this.modalHost = conn.host;
    this.modalUser = conn.user;
    this.modalPort = conn.port;
    this.modalUseKerberos = conn.use_kerberos;
    this.modalBastion = conn.bastion || "";
    this.modalBastionUser = conn.bastion_user || "";
    this.modalKeyPath = conn.key_path || "";
    this.modalTagsString = conn.tags.join(", ");
    this.showModal = true;
  }

  saveConnection = async () => {
    if (!this.modalName.trim() || !this.modalHost.trim()) {
      notify("Name and Host are required.", "error");
      return;
    }

    const tags = this.modalTagsString
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t.length > 0);

    const payload = {
      name: this.modalName.trim(),
      host: this.modalHost.trim(),
      user: this.modalUser.trim() || null,
      port: this.modalPort || null,
      kerberos: this.modalUseKerberos || null,
      bastion: this.modalBastion.trim() || null,
      bastionUser: this.modalBastionUser.trim() || null,
      keyPath: this.modalKeyPath.trim() || null,
      tags,
    };

    try {
      if (this.isEditing) {
        await invoke("edit_connection", {
          id: this.modalConnectionId,
          ...payload,
          user: this.modalUser,
          port: this.modalPort,
          kerberos: this.modalUseKerberos,
        });
        notify("Host updated successfully", "success");
      } else {
        await invoke("add_connection", payload);
        notify("Host added successfully", "success");
      }
      this.showModal = false;
      await this.reloadConnectionsAfterMutation();
      await this.loadStats();
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  deleteConnection = async (conn: Connection) => {
    this.promptDelete(conn.name, `${conn.user}@${conn.host}:${conn.port}`, async () => {
      await invoke("remove_connection", { idOrName: conn.id });
      notify(`'${conn.name}' removed`, "success");
      await this.loadConnections();
      await this.loadStats();
    });
  }

  handleConnect = async (conn: Connection) => {
    this.activeTab = "terminals";
    await tick();
    try {
      await connectSSH(conn);
      requestAnimationFrame(() => fitActiveTerminal());
    } catch (e: unknown) {
      notify(String(e), "error");
    }
  }

  handleTabChange = (tab: AppTab) => {
    this.activeTab = tab;
    if (tab === "terminals") {
      requestAnimationFrame(() => fitActiveTerminal());
    }
    if (tab === "history") {
      void this.loadHistory();
    }
  }

  terminateAllSessions = async () => {
    const count = await closeAllTabs();
    if (count === 0) return;

    await this.loadHistory();
    await this.loadStats();
    notify(`Closed ${count} active session${count === 1 ? "" : "s"}`, "success");
  }

  requestCloseAllSessions = () => {
    const count = terminalState.totalSessionCount;
    if (count === 0) return;

    if (count === 1) {
      void this.terminateAllSessions();
      return;
    }

    this.promptDelete(
      `${count} active or detached sessions`,
      "Every open SSH terminal will be disconnected immediately.",
      this.terminateAllSessions,
      {
        title: "Close all sessions",
        confirmLabel: "Close all",
        warning: "Unsaved work in remote shells may be lost.",
      },
    );
  }

  handleGlobalKeydown = (e: KeyboardEvent) => {
    if (this.showOnboarding || this.showModal || this.showEnvModal) {
      if (e.key === "Escape" && !this.showOnboarding) {
        this.showModal = false;
        this.showEnvModal = false;
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

    if (this.activeTab === "connections" && this.connections.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        this.selectedHostIndex = (this.selectedHostIndex + 1) % this.connections.length;
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        this.selectedHostIndex = (this.selectedHostIndex - 1 + this.connections.length) % this.connections.length;
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (this.connections[this.selectedHostIndex]) this.handleConnect(this.connections[this.selectedHostIndex]);
      } else if (e.key === "e" && e.ctrlKey) {
        e.preventDefault();
        if (this.connections[this.selectedHostIndex]) this.openEditModal(this.connections[this.selectedHostIndex]);
      }
    }
  }
}

export const appState = new AppStateStore();
