/**
 * Settings / workspace / environments / onboarding / SSH-agent domain store.
 *
 * Owns the persisted desktop settings, the workspace info, environment
 * (profile) registry, onboarding flags and the SSH agent status. Cross-cutting
 * orchestration (e.g. `switchEnv` / `createEnv` / `completeOnboarding` which
 * reload the whole app) lives on the `appState` composition class.
 */
import { invoke } from "@tauri-apps/api/core";
import type { DesktopSettings, EnvInfo, WorkspaceInfo } from "$lib/types";
import { notify } from "$lib/stores/notifications.svelte";
import { applyTheme } from "$lib/utils/theme";
import { applyThemeToAllTerminals, registerSettingsGetter } from "$lib/stores/terminal.svelte";
import {
  startKerberosMonitoring,
  stopKerberosMonitoring,
} from "$lib/stores/kerberos.svelte";
import { loadConnections, loadStats } from "$lib/stores/connections.svelte";

let settings = $state<DesktopSettings>({
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
  terminal_font_family: "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace",
  terminal_font_size: 13,
  terminal_line_height: 1.18,
  terminal_cursor_style: "block",
  terminal_cursor_blink: true,
  terminal_copy_on_select: false,
  terminal_scrollback: 10000,
  sftp_show_hidden_files: true,
  sftp_default_remote_path: "/",
  confirm_snippet_execution: true,
  enable_sftp: true,
  enable_tunneling: true,
});

registerSettingsGetter(() => settings);

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
  log_level: "info",
  auto_save_history: true,
  max_history_size: 1000,
});

let environments = $state<EnvInfo[]>([]);
let activeEnv = $state("default");
let newEnvName = $state("");
let showOnboarding = $state(false);
let isInitializing = $state(true);

// SSH agent status (owned here — configured from the Settings view).
let agentActive = $state(false);
let agentSocket = $state<string | null>(null);
let agentKeys = $state<string[]>([]);

export async function loadWorkspace() {
  try {
    workspace = await invoke("get_workspace_info");
    activeEnv = workspace.active_env;
  } catch (e) {
    console.error("Failed to load workspace info", e);
  }
}

export async function saveWorkspaceConfig() {
  try {
    await invoke("save_workspace_config", {
      update: {
        default_user: settings.default_user,
        default_port: settings.default_port,
        ssh_config_path: workspace.ssh_config_path || "",
        search_mode: settings.fuzzy_search ? "fuzzy" : "bayesian",
        log_level: workspace.log_level,
        auto_save_history: workspace.auto_save_history,
        max_history_size: workspace.max_history_size,
      },
    });
    await loadWorkspace();
    notify("Workspace settings saved", "success");
  } catch (e: unknown) {
    notify(`Failed to save workspace: ${e}`, "error");
  }
}

export async function browseSshConfig(): Promise<string | null> {
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

export async function importSshConfig() {
  try {
    const count = await invoke<number>("import_ssh_config", {
      file: workspace.ssh_config_path || null,
    });
    await loadConnections();
    await loadStats();
    try {
      await invoke("refresh_tray_menu");
    } catch (e) {
      console.error(e);
    }
    notify(
      count > 0 ? `Imported ${count} host${count === 1 ? "" : "s"} from OpenSSH config` : "No new hosts to import",
      count > 0 ? "success" : "info",
    );
  } catch (e: unknown) {
    notify(`Import failed: ${e}`, "error");
  }
}

export async function checkOnboarding(): Promise<boolean> {
  try {
    const needsSetup = await invoke<boolean>("needs_onboarding");
    showOnboarding = needsSetup;
    return needsSetup;
  } catch (e) {
    console.error("Failed to check onboarding state", e);
    return false;
  }
}

export async function loadSettings() {
  try {
    const loaded: Record<string, unknown> = await invoke("load_desktop_settings");
    settings = {
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
      terminal_font_family: (loaded.terminal_font_family as string) || "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace",
      terminal_font_size: Number(loaded.terminal_font_size) || 13,
      terminal_line_height: Number(loaded.terminal_line_height) || 1.18,
      terminal_cursor_style: (loaded.terminal_cursor_style as "block" | "bar" | "underline") || "block",
      terminal_cursor_blink: loaded.terminal_cursor_blink !== false,
      terminal_copy_on_select: Boolean(loaded.terminal_copy_on_select),
      terminal_scrollback: Number(loaded.terminal_scrollback) || 10000,
      sftp_show_hidden_files: loaded.sftp_show_hidden_files !== false,
      sftp_default_remote_path: (loaded.sftp_default_remote_path as string) || "/",
      confirm_snippet_execution: loaded.confirm_snippet_execution !== false,
      enable_sftp: loaded.enable_sftp !== false,
      enable_tunneling: loaded.enable_tunneling !== false,
    };
    applyTheme(settings.theme);
    applyThemeToAllTerminals(settings);

    if (settings.auto_start_agent && !agentActive) {
      await triggerStartAgent();
    }

    if (settings.monitor_kerberos) {
      startKerberosMonitoring({
        warnMinutes: settings.kerberos_warn_minutes,
        onWarning: (message) => notify(message, "info"),
      });
    } else {
      stopKerberosMonitoring();
    }
  } catch (e) {
    console.error("Failed to load settings", e);
  }
}

export async function saveSettings() {
  applyTheme(settings.theme);
  applyThemeToAllTerminals(settings);
  try {
    await invoke("save_desktop_settings", {
      settings: { ...settings, onboarding_complete: true },
    });
    if (settings.monitor_kerberos) {
      startKerberosMonitoring({
        warnMinutes: settings.kerberos_warn_minutes,
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

export function handleThemeChange(theme: string) {
  settings.theme = theme;
  applyTheme(theme);
  void saveSettings();
}

export async function loadAgentStatus() {
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

export async function triggerStartAgent() {
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

export async function triggerAddKey(keyPath: string) {
  try {
    await invoke("add_key_to_agent", { keyPath });
    await loadAgentStatus();
    notify("Key added to SSH Agent successfully", "success");
  } catch (e: unknown) {
    notify(`Failed to add key: ${e}`, "error");
  }
}

export async function selectAndAddKey() {
  try {
    const file = await invoke<string | null>("pick_key_file");
    if (file) await triggerAddKey(file);
  } catch (e) {
    console.error("Failed to pick key file", e);
  }
}

export function getSettingsState() {
  return {
    get settings() {
      return settings;
    },
    set settings(value: DesktopSettings) {
      settings = value;
    },
    get workspace() {
      return workspace;
    },
    set workspace(value: WorkspaceInfo) {
      workspace = value;
    },
    get environments() {
      return environments;
    },
    set environments(value: EnvInfo[]) {
      environments = value;
    },
    get activeEnv() {
      return activeEnv;
    },
    set activeEnv(value: string) {
      activeEnv = value;
    },
    get newEnvName() {
      return newEnvName;
    },
    set newEnvName(value: string) {
      newEnvName = value;
    },
    get showOnboarding() {
      return showOnboarding;
    },
    set showOnboarding(value: boolean) {
      showOnboarding = value;
    },
    get isInitializing() {
      return isInitializing;
    },
    set isInitializing(value: boolean) {
      isInitializing = value;
    },
    get agentActive() {
      return agentActive;
    },
    set agentActive(value: boolean) {
      agentActive = value;
    },
    get agentSocket() {
      return agentSocket;
    },
    set agentSocket(value: string | null) {
      agentSocket = value;
    },
    get agentKeys() {
      return agentKeys;
    },
    set agentKeys(value: string[]) {
      agentKeys = value;
    },
    loadWorkspace,
    saveWorkspaceConfig,
    browseSshConfig,
    importSshConfig,
    checkOnboarding,
    loadSettings,
    saveSettings,
    handleThemeChange,
    loadAgentStatus,
    triggerStartAgent,
    triggerAddKey,
    selectAndAddKey,
  };
}
