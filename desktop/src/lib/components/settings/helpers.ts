import type { DesktopSettings, WorkspaceInfo } from "$lib/types";

/**
 * Derived workspace fields are synchronized from the settings form before any
 * workspace save. Mirrors the logic previously inline in SettingsView.svelte.
 */
export function syncWorkspaceFromForm(
  workspace: WorkspaceInfo,
  settings: DesktopSettings,
  sshConfigPath: string
): WorkspaceInfo {
  return {
    ...workspace,
    ssh_config_path: sshConfigPath.trim() || null,
    search_mode: settings.fuzzy_search ? "fuzzy" : "bayesian",
    default_user: settings.default_user,
    default_port: settings.default_port,
  };
}

/**
 * Save the workspace after re-deriving its form-derived fields.
 * Returns the next workspace value; the caller reassigns its `$bindable()` prop.
 */
export function handleWorkspaceSave(
  workspace: WorkspaceInfo,
  settings: DesktopSettings,
  sshConfigPath: string,
  onSaveWorkspace: () => void
): WorkspaceInfo {
  const next = syncWorkspaceFromForm(workspace, settings, sshConfigPath);
  onSaveWorkspace();
  return next;
}

/**
 * Save both defaults and the workspace after re-deriving form-derived fields.
 * Returns the next workspace value; the caller reassigns its `$bindable()` prop.
 */
export function handleDefaultsSave(
  workspace: WorkspaceInfo,
  settings: DesktopSettings,
  sshConfigPath: string,
  onSave: () => void,
  onSaveWorkspace: () => void
): WorkspaceInfo {
  const next = syncWorkspaceFromForm(workspace, settings, sshConfigPath);
  onSave();
  onSaveWorkspace();
  return next;
}
