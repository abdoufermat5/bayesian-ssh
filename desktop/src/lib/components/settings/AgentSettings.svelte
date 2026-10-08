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
    <h2 class="settings-heading">SSH agent</h2>
    <p class="settings-desc">Agent behaviour and the values used when a host doesn't set its own.</p>
  </div>

  <section>
    <h3 class="settings-group-title">Agent</h3>
    <div class="settings-group">
      <label class="setting-row cursor-pointer">
        <span class="setting-row-main">
          <span class="setting-title">Start agent on launch</span>
          <span class="setting-meta">Launch the built-in SSH agent when the app opens.</span>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={settings.auto_start_agent}
          onchange={(e) => {
            settings.auto_start_agent = (e.target as HTMLInputElement).checked;
            onSave();
          }}
        />
      </label>
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="settings-agent-socket">Custom agent socket</label>
          <span class="setting-meta">Use an external agent instead. Leave empty for the built-in one.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-agent-socket"
            type="text"
            placeholder="/tmp/agent.sock"
            value={settings.custom_agent_socket}
            onchange={(e) => {
              settings.custom_agent_socket = (e.target as HTMLInputElement).value;
              onSave();
            }}
            class="input input-mono w-56"
          />
        </div>
      </div>
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">Connection defaults</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="settings-default-user">Default user</label>
          <span class="setting-meta">Used for hosts without a user.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-default-user"
            type="text"
            value={settings.default_user}
            onchange={(e) => {
              settings.default_user = (e.target as HTMLInputElement).value;
              saveDefaults();
            }}
            class="input w-48"
          />
        </div>
      </div>
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="settings-default-port">Default port</label>
          <span class="setting-meta">Used for hosts without a port.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-default-port"
            type="number"
            min="1"
            max="65535"
            value={settings.default_port}
            onchange={(e) => {
              settings.default_port = Number((e.target as HTMLInputElement).value);
              saveDefaults();
            }}
            class="input w-48 tabular-nums"
          />
        </div>
      </div>
    </div>
  </section>
</div>
