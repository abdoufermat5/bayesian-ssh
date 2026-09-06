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
  >
    <div class="modal-header">
        <div class="flex items-center gap-3">
          <div class="icon-tile">
            <TerminalSquare size={18} />
          </div>
          <div>
            <h2 class="modal-title flex items-center gap-2">
              Bayesian SSH
              <span class="tag text-accent">
                {appVersion}
              </span>
            </h2>
            <p class="modal-subtitle">Fast & lightweight SSH session manager with Kerberos support</p>
          </div>
        </div>
        <button
          type="button"
          class="modal-close"
          onclick={onClose}
          title="Close"
          aria-label="Close"
        >
          <X size={16} />
        </button>
      </div>

      <div class="modal-body flex flex-col gap-3">
        <div class="settings-section">
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
              <HardDrive size={13} class="text-accent" />
              Active Profile
            </span>
            <span class="font-mono text-[11px] text-primary">{activeEnv}</span>
          </div>
        </div>

        {#if workspace}
          <div class="system-value space-y-1 break-all">
            <div><span class="text-primary font-sans font-semibold">Config:</span> {workspace.config_root}</div>
            <div><span class="text-primary font-sans font-semibold">Database:</span> {workspace.database_path}</div>
          </div>
        {/if}
      </div>

      <div class="px-5 pb-4 text-xs text-muted leading-relaxed">
        <p class="m-0">
          Crafted for developers who want keyboard-first SSH host management, smart frequency ranking, and zero bloat.
        </p>
      </div>

      <div class="modal-footer justify-between text-[11px] text-muted">
        <span class="flex items-center gap-1">
          Made with <Heart size={11} class="text-error fill-error" /> by Abdoufermat
        </span>
        <button
          type="button"
          class="btn btn-primary"
          onclick={onClose}
        >
          Close
        </button>
      </div>
  </ModalShell>
{/if}
