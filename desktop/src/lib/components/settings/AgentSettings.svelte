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

<div class="flex flex-col gap-6 max-w-2xl">
  <div>
    <h3 class="text-base font-semibold text-primary m-0">SSH Agent & Defaults</h3>
    <p class="text-xs text-muted mt-1">Configure SSH Agent options and fallback connection values</p>
  </div>

  <div class="h-px bg-border/50"></div>

  <div class="flex items-center justify-between gap-4 py-1">
    <div class="flex flex-col gap-0.5">
      <span class="text-xs font-semibold text-secondary">Auto-start SSH Agent</span>
      <span class="text-[11px] text-muted leading-snug">Automatically launch internal agent on desktop app startup</span>
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

  <div class="flex flex-col gap-1.5">
    <label for="settings-agent-socket" class="text-xs font-semibold text-secondary">Custom Agent Socket Path</label>
    <span class="text-[11px] text-muted">Use a custom system socket path instead of the internal agent</span>
    <input
      id="settings-agent-socket"
      type="text"
      placeholder="e.g. /tmp/custom-agent.sock (blank to use default)"
      value={settings.custom_agent_socket}
      onchange={(e) => {
        settings.custom_agent_socket = (e.target as HTMLInputElement).value;
        onSave();
      }}
      class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] mt-1"
    />
  </div>

  <div class="h-px bg-border/50 my-1"></div>

  <div class="flex flex-col gap-4">
    <h4 class="text-[10px] font-bold tracking-widest text-muted uppercase">Fallback Connection Values</h4>

    <div class="grid grid-cols-2 gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="settings-default-user" class="text-xs font-medium text-secondary">Default Username</label>
        <input
          id="settings-default-user"
          type="text"
          value={settings.default_user}
          onchange={(e) => {
            settings.default_user = (e.target as HTMLInputElement).value;
            saveDefaults();
          }}
          class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
        />
      </div>

      <div class="flex flex-col gap-1.5">
        <label for="settings-default-port" class="text-xs font-medium text-secondary">Default Port</label>
        <input
          id="settings-default-port"
          type="number"
          value={settings.default_port}
          onchange={(e) => {
            settings.default_port = Number((e.target as HTMLInputElement).value);
            saveDefaults();
          }}
          class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
        />
      </div>
    </div>
  </div>
</div>
