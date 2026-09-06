<script lang="ts">
  import type { DesktopSettings, WorkspaceInfo } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { handleWorkspaceSave } from "./helpers";

  interface Props {
    settings: DesktopSettings;
    workspace: WorkspaceInfo;
    onSaveWorkspace: () => void;
  }

  let { settings = $bindable(), workspace = $bindable(), onSaveWorkspace }: Props = $props();

  function saveWorkspace() {
    workspace = handleWorkspaceSave(
      workspace,
      settings,
      workspace.ssh_config_path || "",
      onSaveWorkspace
    );
  }
</script>

<div class="settings-page">
  <div>
    <h3 class="settings-heading">Session Logs</h3>
    <p class="settings-desc">Configure automatic history logs and application diagnostic logs</p>
  </div>

  <div class="settings-divider"></div>

  <div class="setting-row">
    <div class="setting-row-main">
      <span class="setting-title">Record Session History</span>
      <span class="setting-meta">Save SSH session connection events to the workspace database</span>
    </div>
    <input
      type="checkbox"
      checked={workspace.auto_save_history}
      onchange={(e) => {
        workspace = {
          ...workspace,
          auto_save_history: (e.target as HTMLInputElement).checked,
        };
        saveWorkspace();
      }}
      class="w-[18px] h-[18px] accent-accent cursor-pointer shrink-0"
    />
  </div>

  <div class="field">
    <label for="settings-max-history" class="field-label">Maximum Log Entries</label>
    <span class="field-meta">Maximum count of historical session records kept in the database</span>
    <input
      id="settings-max-history"
      type="number"
      min="50"
      max="100000"
      step="50"
      bind:value={workspace.max_history_size}
      onchange={saveWorkspace}
      class="input mt-1"
    />
  </div>

  <div class="field">
    <label id="settings-log-level-label" for="settings-log-level" class="field-label">Application Log Level</label>
    <span class="field-meta">Controls backend diagnostic log granularity</span>
    <CustomSelect
      id="settings-log-level"
      options={[
        { value: "trace", label: "Trace" },
        { value: "debug", label: "Debug" },
        { value: "info", label: "Info" },
        { value: "warn", label: "Warn" },
        { value: "error", label: "Error" },
        { value: "off", label: "Off" }
      ]}
      value={workspace.log_level}
      onChange={(val) => {
        workspace.log_level = val;
        saveWorkspace();
      }}
    />
  </div>
</div>
