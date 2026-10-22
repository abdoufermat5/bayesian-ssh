/**
 * Settings sections, and a one-shot request to open one (from the command
 * palette or a link elsewhere). SettingsView takes the request when it shows.
 */

export const SETTINGS_SECTIONS = [
  { id: "workspace", label: "Workspace" },
  { id: "connection", label: "Connection" },
  { id: "terminal", label: "Terminal & files" },
  { id: "privacy", label: "Privacy & history" },
  { id: "appearance", label: "Appearance" },
  { id: "sources", label: "Sources & updates" },
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]["id"];

let pending = $state<SettingsSectionId | null>(null);

/** Ask SettingsView to show this section; the caller switches to the settings tab. */
export function requestSettingsSection(id: SettingsSectionId) {
  pending = id;
}

/** Returns and clears the pending request. */
export function takeSettingsSection(): SettingsSectionId | null {
  const id = pending;
  pending = null;
  return id;
}

export function pendingSettingsSection(): SettingsSectionId | null {
  return pending;
}
