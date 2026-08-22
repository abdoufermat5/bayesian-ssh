<script lang="ts">
  import type { DesktopSettings } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="flex flex-col gap-6 max-w-2xl">
  <div>
    <h3 class="text-base font-semibold text-primary m-0">Terminal Emulation & Appearance</h3>
    <p class="text-xs text-muted mt-1">Configure typography, cursor styles, scrollback history, and interactive options</p>
  </div>

  <div class="h-px bg-border/50"></div>

  <!-- Typography & Font Family -->
  <div class="flex flex-col gap-2">
    <label for="terminal-font-family" class="text-xs font-semibold text-secondary">Font Family</label>
    <span class="text-[11px] text-muted">Primary monospace font stack for terminal windows</span>
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
    <div class="flex flex-col gap-1.5">
      <label for="terminal-font-size" class="text-xs font-semibold text-secondary">Font Size (px)</label>
      <input
        id="terminal-font-size"
        type="number"
        min="8"
        max="28"
        bind:value={settings.terminal_font_size}
        onchange={onSave}
        class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
      />
    </div>

    <div class="flex flex-col gap-1.5">
      <label for="terminal-line-height" class="text-xs font-semibold text-secondary">Line Height</label>
      <input
        id="terminal-line-height"
        type="number"
        step="0.02"
        min="1.0"
        max="1.5"
        bind:value={settings.terminal_line_height}
        onchange={onSave}
        class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
      />
    </div>
  </div>

  <!-- Live Font Preview Box -->
  <div class="flex flex-col gap-1.5 p-3.5 rounded-xl border border-border/70 bg-[#09090b]">
    <span class="text-[10px] font-bold text-muted uppercase tracking-wider mb-1">Live Font Preview</span>
    <div
      class="text-running p-3 rounded-lg bg-black/60 font-mono text-xs overflow-x-auto whitespace-pre select-none border border-white/5"
      style="font-family: {settings.terminal_font_family}; font-size: {settings.terminal_font_size || 13}px; line-height: {settings.terminal_line_height || 1.18};"
    >
      <span class="text-running">user@bayesian-ssh</span>:<span class="text-blue-400">~</span>$ uname -a &amp;&amp; uptime
      <span class="text-muted block mt-0.5">Linux production-srv1 6.8.0-31-generic #31-Ubuntu SMP PREEMPT_DYNAMIC</span>
      <span class="text-warning block"> 04:10:00 up 42 days, 12:34,  2 users,  load average: 0.12, 0.08, 0.04</span>
    </div>
  </div>

  <div class="h-px bg-border/50"></div>

  <!-- Cursor & Interaction Grid -->
  <div class="grid grid-cols-2 gap-4">
    <div class="flex flex-col gap-1.5">
      <label for="terminal-cursor-style" class="text-xs font-semibold text-secondary">Cursor Style</label>
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

    <div class="flex flex-col gap-1.5">
      <label for="terminal-scrollback" class="text-xs font-semibold text-secondary">Scrollback Buffer (Lines)</label>
      <input
        id="terminal-scrollback"
        type="number"
        step="1000"
        min="1000"
        max="50000"
        bind:value={settings.terminal_scrollback}
        onchange={onSave}
        class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
      />
    </div>
  </div>

  <!-- Toggles -->
  <div class="flex flex-col gap-3 pt-1">
    <label class="flex items-center gap-3 cursor-pointer select-none">
      <input
        type="checkbox"
        bind:checked={settings.terminal_cursor_blink}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="text-xs font-semibold text-primary">Smooth Cursor Blinking</span>
        <span class="text-[11px] text-muted">Blink cursor when terminal window is focused</span>
      </div>
    </label>

    <label class="flex items-center gap-3 cursor-pointer select-none">
      <input
        type="checkbox"
        bind:checked={settings.terminal_copy_on_select}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="text-xs font-semibold text-primary">Copy on Select</span>
        <span class="text-[11px] text-muted">Automatically copy highlighted text to system clipboard</span>
      </div>
    </label>

    <label class="flex items-center gap-3 cursor-pointer select-none">
      <input
        type="checkbox"
        bind:checked={settings.sftp_show_hidden_files}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="text-xs font-semibold text-primary">SFTP Show Hidden Files (Dotfiles)</span>
        <span class="text-[11px] text-muted">Display hidden files (`.bashrc`, `.env`) in the SFTP browser</span>
      </div>
    </label>

    <label class="flex items-center gap-3 cursor-pointer select-none">
      <input
        type="checkbox"
        bind:checked={settings.confirm_snippet_execution}
        onchange={onSave}
        class="rounded border-border bg-surface-input text-accent focus:ring-accent/20"
      />
      <div class="flex flex-col">
        <span class="text-xs font-semibold text-primary">Confirm Snippet Execution</span>
        <span class="text-[11px] text-muted">Prompt for confirmation before injecting command snippets into terminal PTY</span>
      </div>
    </label>
  </div>

  <div class="pt-2">
    <button
      type="button"
      class="px-5 py-2.5 rounded-lg bg-accent text-white text-xs font-semibold cursor-pointer hover:opacity-90 transition-all shadow-sm"
      onclick={onSave}
    >
      Save Terminal Settings
    </button>
  </div>
</div>
