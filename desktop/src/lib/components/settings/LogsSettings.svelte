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

  const logLevelOptions = [
    { value: "trace", label: "Trace" },
    { value: "debug", label: "Debug" },
    { value: "info", label: "Info" },
    { value: "warn", label: "Warn" },
    { value: "error", label: "Error" },
    { value: "off", label: "Off" },
  ];

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
    <h2 class="settings-heading">History</h2>
    <p class="settings-desc">Session history and diagnostic logging.</p>
  </div>

  <section>
    <h3 class="settings-group-title">Session history</h3>
    <div class="settings-group">
      <label class="setting-row cursor-pointer">
        <span class="setting-row-main">
          <span class="setting-title">Record session history</span>
          <span class="setting-meta">Save connection events to the profile database.</span>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={workspace.auto_save_history}
          onchange={(e) => {
            workspace = {
              ...workspace,
              auto_save_history: (e.target as HTMLInputElement).checked,
            };
            saveWorkspace();
          }}
        />
      </label>
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="settings-max-history">Maximum entries</label>
          <span class="setting-meta">Older records are removed beyond this count.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-max-history"
            type="number"
            min="50"
            max="100000"
            step="50"
            bind:value={workspace.max_history_size}
            onchange={saveWorkspace}
            class="input w-48 tabular-nums"
          />
        </div>
      </div>
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">Diagnostics</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Log level</span>
          <span class="setting-meta">Verbosity of the backend diagnostic log.</span>
        </div>
        <div class="setting-control">
          <CustomSelect
            id="settings-log-level"
            class="w-48"
            size="md"
            options={logLevelOptions}
            value={workspace.log_level}
            onChange={(val) => {
              workspace.log_level = val;
              saveWorkspace();
            }}
          />
        </div>
      </div>
    </div>
  </section>
</div>
