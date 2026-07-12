<script lang="ts">
  import type { DesktopSettings } from "$lib/types";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="page-view settings-view">
  <div class="page-view-header view-header settings-view-header">
    <div class="title-meta">
      <h2>Desktop Settings</h2>
      <span class="subtitle">Configure preferences for the desktop application</span>
    </div>
  </div>

  <div class="page-view-scroll">
    <div class="settings-grid">
      <div class="settings-card">
        <span class="settings-card-title">APPEARANCE</span>

        <div class="settings-field">
          <label for="settings-theme">Active Theme</label>
          <select id="settings-theme" class="cyber-select" bind:value={settings.theme} onchange={onSave}>
            <option value="zinc">Slate Minimalist (Zinc)</option>
            <option value="cyberpunk">Cyberpunk Neon (Dark Glow)</option>
            <option value="oled">OLED Pitch Black</option>
            <option value="slate">Sleek Navy (Slate)</option>
          </select>
        </div>

        <div class="settings-row">
          <div class="settings-field">
            <span class="label-style">Fuzzy Search Scoring</span>
            <span class="settings-field-hint">Prioritize query match over Bayesian connect frequency</span>
          </div>
          <input
            type="checkbox"
            bind:checked={settings.fuzzy_search}
            onchange={onSave}
            class="settings-checkbox"
          />
        </div>
      </div>

      <div class="settings-card">
        <span class="settings-card-title">SSH AGENT INTEGRATION</span>

        <div class="settings-row">
          <div class="settings-field">
            <span class="label-style">Auto-start SSH Agent</span>
            <span class="settings-field-hint">Start agent automatically on desktop app startup</span>
          </div>
          <input
            type="checkbox"
            bind:checked={settings.auto_start_agent}
            onchange={onSave}
            class="settings-checkbox"
          />
        </div>

        <div class="settings-field">
          <label for="settings-agent-socket">Custom Agent Socket Path</label>
          <input
            id="settings-agent-socket"
            type="text"
            placeholder="e.g. /tmp/custom-agent.sock (blank to use default)"
            bind:value={settings.custom_agent_socket}
            onchange={onSave}
            class="cyber-input"
          />
        </div>
      </div>

      <div class="settings-card">
        <span class="settings-card-title">CONNECTION DEFAULTS</span>

        <div class="settings-field">
          <label for="settings-default-user">Default SSH User</label>
          <input id="settings-default-user" type="text" bind:value={settings.default_user} onchange={onSave} class="cyber-input" />
        </div>

        <div class="settings-field">
          <label for="settings-default-port">Default SSH Port</label>
          <input id="settings-default-port" type="number" bind:value={settings.default_port} onchange={onSave} class="cyber-input" />
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .settings-view-header {
    border-bottom: 1px solid var(--border-color);
    padding-bottom: 1rem;
  }

  .label-style {
    font-size: 12px;
    font-weight: 500;
  }

  .settings-checkbox {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-cyan);
    flex-shrink: 0;
  }
</style>
