<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { TerminalSquare, X, ShieldCheck, Cpu, HardDrive, Info, Heart, ExternalLink } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { WorkspaceInfo } from "$lib/types";

  interface Props {
    show: boolean;
    workspace?: WorkspaceInfo | null;
    activeEnv?: string;
    onClose: () => void;
  }

  let { show, workspace = null, activeEnv = "default", onClose }: Props = $props();

  let appVersion = $state<string>("...");

  onMount(() => {
    (async () => {
      try {
        const ver = await invoke<string>("get_app_version");
        appVersion = `v${ver}`;
      } catch {
        appVersion = "v2.2.0";
      }
    })();
  });
</script>

{#if show}
  <ModalShell
    open={show}
    title="About Bayesian SSH"
    onClose={onClose}
    width="md"
    panelClass="p-6 space-y-5 relative overflow-hidden"
  >
    <!-- Header -->
    <div class="flex items-start justify-between">
        <div class="flex items-center gap-3">
          <div class="w-12 h-12 rounded-xl bg-accent/15 border border-accent/30 flex items-center justify-center shrink-0 shadow-inner">
            <TerminalSquare class="text-accent" size={26} />
          </div>
          <div>
            <h2 class="text-base font-bold text-primary m-0 tracking-tight flex items-center gap-2">
              Bayesian SSH
              <span class="text-[10px] font-mono font-semibold px-2 py-0.5 rounded-full bg-accent/15 text-accent border border-accent/25">
                {appVersion}
              </span>
            </h2>
            <p class="text-xs text-muted mt-0.5 m-0">Fast & lightweight SSH session manager with Kerberos support</p>
          </div>
        </div>
        <button
          type="button"
          class="p-1.5 rounded-lg text-muted hover:text-primary hover:bg-white/10 transition-colors"
          onclick={onClose}
          title="Close"
        >
          <X size={16} />
        </button>
      </div>

      <!-- App Info Grid -->
      <div class="space-y-2 text-xs">
        <div class="p-3 rounded-xl bg-surface-input/60 border border-border space-y-2">
          <div class="flex justify-between items-center text-muted">
            <span class="flex items-center gap-1.5 font-medium">
              <Cpu size={13} class="text-accent" />
              Core Engine
            </span>
            <span class="font-mono text-[11px] text-primary">bayesian-ssh-core {appVersion}</span>
          </div>

          <div class="flex justify-between items-center text-muted">
            <span class="flex items-center gap-1.5 font-medium">
              <ShieldCheck size={13} class="text-running" />
              Security Suite
            </span>
            <span class="font-mono text-[11px] text-primary">PBKDF2 + AES-GCM + POSIX 0600</span>
          </div>

          <div class="flex justify-between items-center text-muted">
            <span class="flex items-center gap-1.5 font-medium">
              <HardDrive size={13} class="text-cyan-400" />
              Active Profile
            </span>
            <span class="font-mono text-[11px] text-primary">{activeEnv}</span>
          </div>
        </div>

        {#if workspace}
          <div class="p-3 rounded-xl bg-surface-input/40 border border-border/60 text-[11px] text-muted space-y-1 font-mono break-all">
            <div><span class="text-primary font-sans font-semibold">Config:</span> {workspace.config_root}</div>
            <div><span class="text-primary font-sans font-semibold">Database:</span> {workspace.database_path}</div>
          </div>
        {/if}
      </div>

      <!-- Description & Credits -->
      <div class="text-xs text-muted leading-relaxed border-t border-border pt-3">
        <p class="m-0">
          Crafted for developers who want keyboard-first SSH host management, smart frequency ranking, and zero bloat.
        </p>
      </div>

      <!-- Footer -->
      <div class="pt-2 border-t border-border flex justify-between items-center text-[11px] text-muted">
        <span class="flex items-center gap-1">
          Made with <Heart size={11} class="text-error fill-rose-400" /> by Abdoufermat
        </span>
        <button
          type="button"
          class="px-4 py-1.5 rounded-lg bg-accent text-white text-xs font-semibold hover:opacity-90 transition-all"
          onclick={onClose}
        >
          Close
        </button>
      </div>
  </ModalShell>
{/if}
