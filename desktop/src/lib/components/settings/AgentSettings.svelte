<script lang="ts">
  import type { DesktopSettings, WorkspaceInfo } from "$lib/types";
  import { handleDefaultsSave } from "./helpers";

  interface Props {
    settings: DesktopSettings;
    workspace: WorkspaceInfo;
    onSave: () => void;
    onSaveWorkspace: () => void;
  }

  let {
    settings = $bindable(),
    workspace = $bindable(),
    onSave,
    onSaveWorkspace,
  }: Props = $props();

  function saveDefaults() {
    workspace = handleDefaultsSave(
      workspace,
      settings,
      workspace.ssh_config_path || "",
      onSave,
      onSaveWorkspace
    );
  }
</script>

<div class="settings-page">
  <div>
    <h3 class="settings-heading">SSH Agent & Defaults</h3>
    <p class="settings-desc">Configure SSH Agent options and fallback connection values</p>
  </div>

  <div class="settings-divider"></div>

  <div class="setting-row">
    <div class="setting-row-main">
      <span class="setting-title">Auto-start SSH Agent</span>
      <span class="setting-meta">Automatically launch internal agent on desktop app startup</span>
    </div>
    <input
      type="checkbox"
      checked={settings.auto_start_agent}
      onchange={(e) => {
        settings.auto_start_agent = (e.target as HTMLInputElement).checked;
        onSave();
      }}
      class="w-[18px] h-[18px] accent-accent cursor-pointer shrink-0"
    />
  </div>

  <div class="field">
    <label for="settings-agent-socket" class="field-label">Custom Agent Socket Path</label>
    <span class="field-meta">Use a custom system socket path instead of the internal agent</span>
    <input
      id="settings-agent-socket"
      type="text"
      placeholder="e.g. /tmp/custom-agent.sock (blank to use default)"
      value={settings.custom_agent_socket}
      onchange={(e) => {
        settings.custom_agent_socket = (e.target as HTMLInputElement).value;
        onSave();
      }}
      class="input mt-1"
    />
  </div>

  <div class="settings-divider my-1"></div>

  <div class="settings-section">
    <h4 class="settings-section-title">Fallback Connection Values</h4>

    <div class="grid grid-cols-2 gap-4">
      <div class="field">
        <label for="settings-default-user" class="field-label">Default Username</label>
        <input
          id="settings-default-user"
          type="text"
          value={settings.default_user}
          onchange={(e) => {
            settings.default_user = (e.target as HTMLInputElement).value;
            saveDefaults();
          }}
          class="input"
        />
      </div>

      <div class="field">
        <label for="settings-default-port" class="field-label">Default Port</label>
        <input
          id="settings-default-port"
          type="number"
          value={settings.default_port}
          onchange={(e) => {
            settings.default_port = Number((e.target as HTMLInputElement).value);
            saveDefaults();
          }}
          class="input"
        />
      </div>
    </div>
  </div>
</div>
