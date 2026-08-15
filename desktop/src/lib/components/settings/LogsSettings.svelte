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

<div class="flex flex-col gap-6 max-w-2xl">
  <div>
    <h3 class="text-base font-semibold text-primary m-0">Session Logs</h3>
    <p class="text-xs text-muted mt-1">Configure automatic history logs and application diagnostic logs</p>
  </div>

  <div class="h-px bg-border/50"></div>

  <div class="flex items-center justify-between gap-4 py-1">
    <div class="flex flex-col gap-0.5">
      <span class="text-xs font-semibold text-secondary">Record Session History</span>
      <span class="text-[11px] text-muted leading-snug">Save SSH session connection events to the workspace database</span>
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

  <div class="flex flex-col gap-1.5">
    <label for="settings-max-history" class="text-xs font-semibold text-secondary">Maximum Log Entries</label>
    <span class="text-[11px] text-muted">Maximum count of historical session records kept in the database</span>
    <input
      id="settings-max-history"
      type="number"
      min="50"
      max="100000"
      step="50"
      bind:value={workspace.max_history_size}
      onchange={saveWorkspace}
      class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
    />
  </div>

  <div class="flex flex-col gap-1.5">
    <label id="settings-log-level-label" for="settings-log-level" class="text-xs font-semibold text-secondary">Application Log Level</label>
    <span class="text-[11px] text-muted">Controls backend diagnostic log granularity</span>
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
