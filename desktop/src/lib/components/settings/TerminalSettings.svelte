<script lang="ts">
  import type { DesktopSettings } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();

  const DEFAULT_FONT =
    "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace";

  const fontOptions = [
    { value: DEFAULT_FONT, label: "JetBrains Mono" },
    { value: "Fira Code, JetBrains Mono, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace", label: "Fira Code" },
    { value: "Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace", label: "Cascadia Code" },
    { value: "Hack, Ubuntu Mono, DejaVu Sans Mono, monospace", label: "Hack" },
    { value: "Consolas, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, monospace", label: "Consolas" },
    { value: "Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, monospace", label: "Ubuntu Mono" },
  ];

  const cursorOptions = [
    { value: "block", label: "Block" },
    { value: "bar", label: "Bar" },
    { value: "underline", label: "Underline" },
  ];
</script>

<div class="settings-page">
  <div>
    <h2 class="settings-heading">Terminal</h2>
    <p class="settings-desc">Typography, cursor and behaviour of terminal sessions.</p>
  </div>

  <section>
    <h3 class="settings-group-title">Font</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Font family</span>
          <span class="setting-meta">Falls back to the next installed monospace font.</span>
        </div>
        <div class="setting-control">
          <CustomSelect
            id="terminal-font-family"
            class="w-48"
            size="md"
            options={fontOptions}
            value={settings.terminal_font_family ?? DEFAULT_FONT}
            onChange={(val) => {
              settings.terminal_font_family = val;
              onSave();
            }}
          />
        </div>
      </div>
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="terminal-font-size">Font size</label>
          <span class="setting-meta">In pixels, 8–28.</span>
        </div>
        <div class="setting-control">
          <input
            id="terminal-font-size"
            type="number"
            min="8"
            max="28"
            bind:value={settings.terminal_font_size}
            onchange={onSave}
            class="input w-24 tabular-nums"
          />
        </div>
      </div>
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="terminal-line-height">Line height</label>
          <span class="setting-meta">Multiplier of the font size, 1.0–1.5.</span>
        </div>
        <div class="setting-control">
          <input
            id="terminal-line-height"
            type="number"
            step="0.02"
            min="1.0"
            max="1.5"
            bind:value={settings.terminal_line_height}
            onchange={onSave}
            class="input w-24 tabular-nums"
          />
        </div>
      </div>
      <div class="px-4 py-3">
        <div
          class="overflow-x-auto rounded-md border border-border-subtle bg-surface-terminal p-3 font-mono whitespace-nowrap text-secondary select-none [&>div]:whitespace-pre"
          style="font-family: {settings.terminal_font_family}; font-size: {settings.terminal_font_size || 13}px; line-height: {settings.terminal_line_height || 1.18};"
        >
          <div><span class="text-accent">user@prod-01</span>:<span class="text-primary">~</span>$ uname -a && uptime</div>
          <div class="text-muted">Linux prod-01 6.8.0-31-generic #31-Ubuntu SMP PREEMPT_DYNAMIC x86_64</div>
          <div class="text-muted">{" 04:10:00 up 42 days, 12:34,  2 users,  load average: 0.12, 0.08, 0.04"}</div>
        </div>
      </div>
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">Cursor</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Cursor style</span>
          <span class="setting-meta">Shape of the terminal cursor.</span>
        </div>
        <div class="setting-control">
          <CustomSelect
            id="terminal-cursor-style"
            class="w-48"
            size="md"
            options={cursorOptions}
            value={settings.terminal_cursor_style ?? "block"}
            onChange={(val) => {
              settings.terminal_cursor_style = val as "block" | "bar" | "underline";
              onSave();
            }}
          />
        </div>
      </div>
      <label class="setting-row cursor-pointer">
        <span class="setting-row-main">
          <span class="setting-title">Blink cursor</span>
          <span class="setting-meta">Blink while the terminal has focus.</span>
        </span>
        <input type="checkbox" class="switch" bind:checked={settings.terminal_cursor_blink} onchange={onSave} />
      </label>
    </div>
  </section>

  <section>
    <h3 class="settings-group-title">Behaviour</h3>
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="terminal-scrollback">Scrollback</label>
          <span class="setting-meta">Lines kept in memory per session, 1,000–50,000.</span>
        </div>
        <div class="setting-control">
          <input
            id="terminal-scrollback"
            type="number"
            step="1000"
            min="1000"
            max="50000"
            bind:value={settings.terminal_scrollback}
            onchange={onSave}
            class="input w-24 tabular-nums"
          />
        </div>
      </div>
      <label class="setting-row cursor-pointer">
        <span class="setting-row-main">
          <span class="setting-title">Copy on select</span>
          <span class="setting-meta">Copy selected text to the clipboard automatically.</span>
        </span>
        <input type="checkbox" class="switch" bind:checked={settings.terminal_copy_on_select} onchange={onSave} />
      </label>
      <label class="setting-row cursor-pointer">
        <span class="setting-row-main">
          <span class="setting-title">Confirm snippets</span>
          <span class="setting-meta">Ask before sending a snippet to the terminal.</span>
        </span>
        <input type="checkbox" class="switch" bind:checked={settings.confirm_snippet_execution} onchange={onSave} />
      </label>
      <label class="setting-row cursor-pointer">
        <span class="setting-row-main">
          <span class="setting-title">Show hidden files in SFTP</span>
          <span class="setting-meta">List dotfiles such as .bashrc and .env in the file browser.</span>
        </span>
        <input type="checkbox" class="switch" bind:checked={settings.sftp_show_hidden_files} onchange={onSave} />
      </label>
    </div>
  </section>
</div>
