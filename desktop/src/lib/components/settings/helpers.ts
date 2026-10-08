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
 *
 * The synced fields are written back into `workspace` *before* the save runs:
 * `onSaveWorkspace` reads the app store's workspace, which is the very object
 * passed in by `bind:workspace`, so assigning after the callback would persist
 * the stale value (e.g. a freshly typed SSH config path).
 *
 * Returns the workspace value; the caller reassigns its `$bindable()` prop.
 */
export function handleWorkspaceSave(
  workspace: WorkspaceInfo,
  settings: DesktopSettings,
  sshConfigPath: string,
  onSaveWorkspace: () => void
): WorkspaceInfo {
  Object.assign(workspace, syncWorkspaceFromForm(workspace, settings, sshConfigPath));
  onSaveWorkspace();
  return workspace;
}

/**
 * Save both defaults and the workspace after re-deriving form-derived fields.
 *
 * See `handleWorkspaceSave` for why the workspace is mutated in place before
 * the callbacks fire.
 */
export function handleDefaultsSave(
  workspace: WorkspaceInfo,
  settings: DesktopSettings,
  sshConfigPath: string,
  onSave: () => void,
  onSaveWorkspace: () => void
): WorkspaceInfo {
  Object.assign(workspace, syncWorkspaceFromForm(workspace, settings, sshConfigPath));
  onSave();
  onSaveWorkspace();
  return workspace;
}
