<script lang="ts">
  import type { DesktopSettings } from "$lib/types";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();
</script>

<div class="flex flex-col gap-6 max-w-2xl">
  <div>
    <h3 class="text-base font-semibold text-primary m-0">Features</h3>
    <p class="text-xs text-muted mt-1">Enable or disable optional sections of the application. Disabled sections are hidden from the sidebar.</p>
  </div>

  <div class="h-px bg-border/50"></div>

  <div class="flex flex-col gap-3">
    <!-- SFTP Browser toggle -->
    <div class="flex items-center justify-between gap-4 p-4 rounded-xl border border-border bg-surface-input/40 hover:bg-surface-input/60 transition-colors">
      <div class="flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-accent/10 flex items-center justify-center shrink-0">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-accent">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
          </svg>
        </div>
        <div>
          <p class="text-sm font-semibold text-primary m-0">SFTP File Browser</p>
          <p class="text-[11px] text-muted m-0">Browse and manage remote files over SSH using the built-in SFTP browser</p>
        </div>
      </div>
      <button
        type="button"
        role="switch"
        aria-label="Toggle SFTP File Browser"
        aria-checked={settings.enable_sftp !== false}
        onclick={() => { settings.enable_sftp = settings.enable_sftp === false ? true : false; onSave(); }}
        class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent/30
          {settings.enable_sftp !== false ? 'bg-accent' : 'bg-surface-input border border-border'}"
      >
        <span
          class="pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow transition duration-200 ease-in-out
            {settings.enable_sftp !== false ? 'translate-x-5' : 'translate-x-0'}"
        ></span>
      </button>
    </div>

    <!-- Tunnel Studio toggle -->
    <div class="flex items-center justify-between gap-4 p-4 rounded-xl border border-border bg-surface-input/40 hover:bg-surface-input/60 transition-colors">
      <div class="flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-accent/10 flex items-center justify-center shrink-0">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-accent">
            <path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
            <polyline points="9 22 9 12 15 12 15 22"/>
          </svg>
        </div>
        <div>
          <p class="text-sm font-semibold text-primary m-0">Tunnel Studio</p>
          <p class="text-[11px] text-muted m-0">Manage SSH local port-forward tunnels and SOCKS5 dynamic proxies</p>
        </div>
      </div>
      <button
        type="button"
        role="switch"
        aria-label="Toggle Tunnel Studio"
        aria-checked={settings.enable_tunneling !== false}
        onclick={() => { settings.enable_tunneling = settings.enable_tunneling === false ? true : false; onSave(); }}
        class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent/30
          {settings.enable_tunneling !== false ? 'bg-accent' : 'bg-surface-input border border-border'}"
      >
        <span
          class="pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow transition duration-200 ease-in-out
            {settings.enable_tunneling !== false ? 'translate-x-5' : 'translate-x-0'}"
        ></span>
      </button>
    </div>
  </div>

  <p class="text-[11px] text-muted/70 italic">
    Changes take effect immediately — no restart required.
  </p>
</div>
