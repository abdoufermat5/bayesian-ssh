<script lang="ts">
  import type { DesktopSettings } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="settings-page">
  <div>
    <h3 class="settings-heading">Terminal Emulation & Appearance</h3>
    <p class="settings-desc">Configure typography, cursor styles, scrollback history, and interactive options</p>
  </div>

  <div class="settings-divider"></div>

  <div class="field">
    <label for="terminal-font-family" class="field-label">Font Family</label>
    <span class="field-meta">Primary monospace font stack for terminal windows</span>
    <CustomSelect
      id="terminal-font-family"
      options={[
        { value: "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace", label: "JetBrains Mono (Recommended)" },
        { value: "Fira Code, JetBrains Mono, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace", label: "Fira Code" },
        { value: "Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace", label: "Cascadia Code" },
        { value: "Hack, Ubuntu Mono, DejaVu Sans Mono, monospace", label: "Hack" },
        { value: "Consolas, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, monospace", label: "Consolas" },
        { value: "Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, monospace", label: "Ubuntu Mono" }
      ]}
      value={settings.terminal_font_family ?? "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace"}
      onChange={(val) => {
        settings.terminal_font_family = val;
        onSave();
      }}
    />
  </div>

  <!-- Font Size & Line Height Grid -->
  <div class="grid grid-cols-2 gap-4">
    <div class="field">
      <label for="terminal-font-size" class="field-label">Font Size (px)</label>
      <input
        id="terminal-font-size"
        type="number"
        min="8"
        max="28"
        bind:value={settings.terminal_font_size}
        onchange={onSave}
        class="input"
      />
    </div>

    <div class="field">
      <label for="terminal-line-height" class="field-label">Line Height</label>
      <input
        id="terminal-line-height"
        type="number"
        step="0.02"
        min="1.0"
        max="1.5"
        bind:value={settings.terminal_line_height}
        onchange={onSave}
        class="input"
      />
    </div>
  </div>

  <div class="settings-section bg-surface-terminal">
    <span class="settings-section-title">Live Font Preview</span>
    <div
      class="overflow-x-auto whitespace-pre rounded-md border border-border bg-surface-terminal p-3 font-mono text-xs leading-relaxed text-secondary select-none"
      style="font-family: {settings.terminal_font_family}; font-size: {settings.terminal_font_size || 13}px; line-height: {settings.terminal_line_height || 1.18};"
    >
      <span class="text-accent">user@bayesian-ssh</span>:<span class="text-primary">~</span>$ uname -a &amp;&amp; uptime
      <span class="text-muted block mt-0.5">Linux production-srv1 6.8.0-31-generic #31-Ubuntu SMP PREEMPT_DYNAMIC</span>
      <span class="text-muted block"> 04:10:00 up 42 days, 12:34,  2 users,  load average: 0.12, 0.08, 0.04</span>
    </div>
  </div>

  <div class="settings-divider"></div>

  <div class="grid grid-cols-2 gap-4">
    <div class="field">
      <label for="terminal-cursor-style" class="field-label">Cursor Style</label>
      <CustomSelect
        id="terminal-cursor-style"
        options={[
          { value: "block", label: "Block ( █ )" },
          { value: "bar", label: "Bar ( | )" },
          { value: "underline", label: "Underline ( _ )" }
        ]}
        value={settings.terminal_cursor_style ?? "block"}
        onChange={(val) => {
          settings.terminal_cursor_style = val as "block" | "bar" | "underline";
          onSave();
        }}
      />
    </div>

    <div class="field">
      <label for="terminal-scrollback" class="field-label">Scrollback Buffer (Lines)</label>
      <input
        id="terminal-scrollback"
        type="number"
        step="1000"
        min="1000"
        max="50000"
        bind:value={settings.terminal_scrollback}
        onchange={onSave}
        class="input"
      />
    </div>
  </div>

  <div class="settings-section">
    <label class="flex cursor-pointer items-center gap-3 select-none">
      <input
        type="checkbox"
        bind:checked={settings.terminal_cursor_blink}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="setting-title">Smooth Cursor Blinking</span>
        <span class="setting-meta">Blink cursor when terminal window is focused</span>
      </div>
    </label>

    <label class="flex cursor-pointer items-center gap-3 select-none">
      <input
        type="checkbox"
        bind:checked={settings.terminal_copy_on_select}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="setting-title">Copy on Select</span>
        <span class="setting-meta">Automatically copy highlighted text to system clipboard</span>
      </div>
    </label>

    <label class="flex cursor-pointer items-center gap-3 select-none">
      <input
        type="checkbox"
        bind:checked={settings.sftp_show_hidden_files}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="setting-title">SFTP Show Hidden Files (Dotfiles)</span>
        <span class="setting-meta">Display hidden files (`.bashrc`, `.env`) in the SFTP browser</span>
      </div>
    </label>

    <label class="flex cursor-pointer items-center gap-3 select-none">
      <input
        type="checkbox"
        bind:checked={settings.confirm_snippet_execution}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="setting-title">Confirm Snippet Execution</span>
        <span class="setting-meta">Prompt for confirmation before injecting command snippets into terminal PTY</span>
      </div>
    </label>
  </div>

  <div class="pt-2">
    <button
      type="button"
      class="btn btn-primary"
      onclick={onSave}
    >
      Save Terminal Settings
    </button>
  </div>
</div>
