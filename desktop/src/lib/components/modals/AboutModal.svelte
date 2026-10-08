<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { X, ExternalLink } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { WorkspaceInfo } from "$lib/types";

  interface Props {
    show: boolean;
    workspace?: WorkspaceInfo | null;
    activeEnv?: string;
    onClose: () => void;
  }

  let { show, workspace = null, activeEnv = "default", onClose }: Props = $props();

  const REPO = "https://github.com/abdoufermat5/bayesian-ssh";
  const LINKS = [
    { label: "Source code", href: REPO },
    { label: "Release notes", href: `${REPO}/releases` },
    { label: "Report an issue", href: `${REPO}/issues/new` },
  ];

  let appVersion = $state<string | null>(null);
  let versionRequested = false;

  // Fetch the version lazily the first time the dialog opens.
  $effect(() => {
    if (!show || versionRequested) return;
    versionRequested = true;
    invoke<string>("get_app_version")
      .then((v) => (appVersion = v))
      .catch(() => (appVersion = "unknown"));
  });

  function open(href: string) {
    openUrl(href).catch(() => window.open(href, "_blank", "noopener,noreferrer"));
  }
</script>

{#if show}
  <ModalShell open={show} title="About Bayesian SSH" {onClose} width="md">
    <div class="modal-header">
      <div class="flex min-w-0 items-center gap-3">
        <svg viewBox="0 0 20 20" class="size-9 shrink-0" aria-hidden="true">
          <rect width="20" height="20" rx="5" fill="var(--color-accent)" />
          <path d="M5.5 7l3 3-3 3" fill="none" stroke="var(--color-on-accent)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
          <path d="M10.5 13.25h4" stroke="var(--color-on-accent)" stroke-width="1.6" stroke-linecap="round" />
        </svg>
        <div class="min-w-0">
          <h2 class="modal-title">Bayesian SSH</h2>
          <p class="modal-subtitle tabular-nums">
            {#if appVersion}Version {appVersion}{:else}<span class="skeleton inline-block h-3 w-16 align-middle"></span>{/if}
          </p>
        </div>
      </div>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-4">
      <p class="m-0 text-sm leading-relaxed text-secondary">
        A keyboard-first SSH host manager. It ranks hosts by how often and how recently you use them,
        and supports jump hosts, Kerberos, tunnels and file transfer.
      </p>

      <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 rounded-md border border-border bg-panel px-3 py-2.5 text-xs">
        <dt class="text-muted">Profile</dt>
        <dd class="m-0 truncate font-mono text-secondary">{activeEnv}</dd>
        {#if workspace}
          <dt class="text-muted">Config</dt>
          <dd class="m-0 truncate font-mono text-secondary" title={workspace.config_root}>{workspace.config_root}</dd>
          <dt class="text-muted">Database</dt>
          <dd class="m-0 truncate font-mono text-secondary" title={workspace.database_path}>{workspace.database_path}</dd>
        {/if}
        <dt class="text-muted">License</dt>
        <dd class="m-0 text-secondary">MIT</dd>
      </dl>

      <div class="flex flex-wrap gap-x-4 gap-y-1">
        {#each LINKS as link (link.href)}
          <a
            class="link inline-flex items-center gap-1 text-sm"
            href={link.href}
            target="_blank"
            rel="noreferrer"
            onclick={(e) => {
              e.preventDefault();
              open(link.href);
            }}
          >
            {link.label}
            <ExternalLink size={12} />
          </a>
        {/each}
      </div>
    </div>

    <div class="modal-footer justify-between">
      <span class="text-xs text-muted">Made by Abdoufermat</span>
      <button type="button" class="btn btn-secondary" onclick={onClose}>Close</button>
    </div>
  </ModalShell>
{/if}
