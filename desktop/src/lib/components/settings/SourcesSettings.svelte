<script lang="ts">
  import { onMount } from "svelte";
  import { Eye, Plus, RefreshCw, Trash2, X } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Source, SourceKind, SyncReport } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { formatRelative } from "$lib/utils/timezone";

  let sources = $state<Source[]>([]);
  let loading = $state(true);
  let busy = $state<string | null>(null);
  let reports = $state<Record<string, SyncReport>>({});
  let showAdd = $state(false);
  let removeTarget = $state<Source | null>(null);
  let deleteHosts = $state(false);

  let name = $state("");
  let kind = $state<SourceKind>("ssh-config");
  let path = $state("");
  let inventory = $state("");
  let profile = $state("");
  let regions = $state("");
  let address = $state("");
  let project = $state("");
  let user = $state("");
  let bastion = $state("");
  let includeOffline = $state(false);

  const kindOptions = [
    { value: "ssh-config", label: "SSH config", description: "Hosts from an OpenSSH config file" },
    { value: "ansible", label: "Ansible", description: "Ansible inventory" },
    { value: "tailscale", label: "Tailscale", description: "Devices in your tailnet" },
    { value: "aws", label: "AWS EC2", description: "Running instances" },
    { value: "gcp", label: "Google Cloud", description: "Running Compute Engine instances" },
  ];

  const awsAddressOptions = [
    { value: "private", label: "Private IP", description: "Default" },
    { value: "public", label: "Public IP" },
    { value: "dns", label: "Public DNS name" },
  ];
  const gcpAddressOptions = [
    { value: "internal", label: "Internal IP", description: "Default" },
    { value: "external", label: "External IP" },
  ];

  const kindLabel = (k: SourceKind) =>
    k === "team" ? "Team" : (kindOptions.find((o) => o.value === k)?.label ?? k);
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

  // Required fields per kind, so "Add source" can't create a source that
  // fails on its first sync.
  const canAdd = $derived(
    name.trim() !== "" &&
      (kind !== "ansible" || inventory.trim() !== "") &&
      (kind !== "gcp" || project.trim() !== ""),
  );

  function reportChips(report: SyncReport): Array<{ label: string; cls: string }> {
    const chips = [
      { n: report.added, label: `${report.added} new`, cls: "badge-success" },
      { n: report.updated, label: `${report.updated} updated`, cls: "badge-neutral" },
      { n: report.unstaled, label: `${report.unstaled} back`, cls: "badge-success" },
      { n: report.stale, label: `${report.stale} missing → stale`, cls: "badge-warning" },
      { n: report.pruned, label: `${report.pruned} pruned`, cls: "badge-error" },
      { n: report.skipped_invalid, label: `${report.skipped_invalid} skipped (invalid)`, cls: "badge-neutral" },
    ];
    return chips.filter((c) => c.n > 0);
  }

  onMount(load);

  async function load() {
    loading = true;
    try {
      sources = (await invoke<Source[] | null>("list_sources")) ?? [];
    } catch (error) {
      notify(`Failed to list sources: ${error}`, "error");
    } finally {
      loading = false;
    }
  }

  function resetForm() {
    name = "";
    kind = "ssh-config";
    path = "";
    inventory = "";
    profile = "";
    regions = "";
    address = "";
    project = "";
    user = "";
    bastion = "";
    includeOffline = false;
  }

  async function addSource() {
    const config: Record<string, unknown> = {};
    if (kind === "ssh-config" && path) config.path = path;
    if (kind === "ansible" && inventory) config.inventory = inventory;
    if (kind === "tailscale" && includeOffline) config.include_offline = true;
    if (kind === "aws") {
      config.regions = regions
        .split(",")
        .map((region) => region.trim())
        .filter(Boolean);
      if (profile) config.profile = profile;
      if (address) config.address = address;
    }
    if (kind === "gcp") {
      config.project = project;
      if (address) config.address = address;
    }
    if (user) config.user = user;
    if (bastion) config.bastion = bastion;

    try {
      await invoke<Source>("add_source", { name, kind, config });
      notify(`Added source ${name}`, "success");
      showAdd = false;
      resetForm();
      await load();
    } catch (error) {
      notify(`Failed to add source: ${error}`, "error");
    }
  }

  async function syncSource(source: Source, dryRun: boolean) {
    busy = source.name;
    try {
      const report = await invoke<SyncReport>("sync_source", {
        name: source.name,
        dryRun,
      });
      reports = { ...reports, [source.name]: report };
      notify(dryRun ? `Preview ready for ${source.name}` : `Synced ${source.name}`, "success");
      if (!dryRun) await load();
    } catch (error) {
      notify(`Sync failed: ${error}`, "error");
    } finally {
      busy = null;
    }
  }

  async function removeSource(source: Source) {
    try {
      await invoke("remove_source", { name: source.name, deleteHosts });
      notify(`Removed source ${source.name}`, "success");
      removeTarget = null;
      deleteHosts = false;
      await load();
    } catch (error) {
      notify(`Failed to remove source: ${error}`, "error");
    }
  }
</script>

<div class="settings-page">
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="min-w-0 flex-1 basis-72">
      <h2 class="settings-group-title !mb-1">Inventory sources</h2>
      <p class="setting-meta m-0">
        Sync hosts from SSH configs, Ansible, Tailscale, AWS or GCP. Connections are matched by
        their source id, so a changed address updates the same host.
      </p>
    </div>
    <button type="button" class="btn btn-secondary" onclick={() => (showAdd = !showAdd)}>
      {#if showAdd}<X size={14} /> Cancel{:else}<Plus size={14} /> Add source{/if}
    </button>
  </div>

  {#if showAdd}
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <label class="setting-title" for="source-name">Name</label>
          <span class="setting-meta">Unique name used by `bssh source sync`.</span>
        </div>
        <div class="setting-control">
          <input id="source-name" type="text" class="input w-56" bind:value={name} placeholder="aws-prod" />
        </div>
      </div>

      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">Kind</span>
          <span class="setting-meta">Where hosts are discovered from.</span>
        </div>
        <div class="setting-control">
          <CustomSelect
            class="w-56"
            size="md"
            options={kindOptions}
            value={kind}
            onChange={(value) => (kind = value as SourceKind)}
          />
        </div>
      </div>

      {#if kind === "ssh-config"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Config file</span>
            <span class="setting-meta">Defaults to ~/.ssh/config.</span>
          </div>
          <div class="setting-control">
            <input type="text" class="input input-mono w-56" bind:value={path} placeholder="~/.ssh/config" />
          </div>
        </div>
      {:else if kind === "ansible"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Inventory path</span>
            <span class="setting-meta">Passed to `ansible-inventory -i`.</span>
          </div>
          <div class="setting-control">
            <input type="text" class="input input-mono w-56" bind:value={inventory} placeholder="./inventory.yml" />
          </div>
        </div>
      {:else if kind === "tailscale"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Include offline peers</span>
            <span class="setting-meta">Offline devices are skipped by default.</span>
          </div>
          <div class="setting-control">
            <input type="checkbox" class="switch" bind:checked={includeOffline} />
          </div>
        </div>
      {:else if kind === "aws"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Regions</span>
            <span class="setting-meta">Comma-separated, e.g. eu-west-1,us-east-1.</span>
          </div>
          <div class="setting-control">
            <input type="text" class="input w-56" bind:value={regions} placeholder="eu-west-1" />
          </div>
        </div>
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Profile</span>
            <span class="setting-meta">Optional AWS CLI profile.</span>
          </div>
          <div class="setting-control">
            <input type="text" class="input w-56" bind:value={profile} placeholder="default" />
          </div>
        </div>
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Address</span>
            <span class="setting-meta">Which address of each instance to connect to.</span>
          </div>
          <div class="setting-control">
            <CustomSelect
              class="w-56"
              size="md"
              options={awsAddressOptions}
              value={address || "private"}
              onChange={(value) => (address = value)}
            />
          </div>
        </div>
      {:else if kind === "gcp"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Project</span>
            <span class="setting-meta">GCP project id.</span>
          </div>
          <div class="setting-control">
            <input type="text" class="input w-56" bind:value={project} placeholder="my-project" />
          </div>
        </div>
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Address</span>
            <span class="setting-meta">Which address of each instance to connect to.</span>
          </div>
          <div class="setting-control">
            <CustomSelect
              class="w-56"
              size="md"
              options={gcpAddressOptions}
              value={address || "internal"}
              onChange={(value) => (address = value)}
            />
          </div>
        </div>
      {/if}

      {#if kind === "aws" || kind === "gcp"}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">SSH user</span>
            <span class="setting-meta">Falls back to the app default user.</span>
          </div>
          <div class="setting-control">
            <input type="text" class="input w-56" bind:value={user} placeholder="deploy" />
          </div>
        </div>
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Bastion</span>
            <span class="setting-meta">Applied to every host from this source.</span>
          </div>
          <div class="setting-control">
            <input type="text" class="input w-56" bind:value={bastion} placeholder="bastion.example.com" />
          </div>
        </div>
      {/if}

      <div class="setting-row">
        <span class="setting-row-main"></span>
        <div class="setting-control">
          <button type="button" class="btn btn-primary" onclick={addSource} disabled={!canAdd}>
            <Plus size={14} />
            Add source
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if loading}
    <p class="text-sm text-muted">Loading sources…</p>
  {:else if sources.length === 0}
    <div class="settings-group">
      <div class="setting-row">
        <div class="setting-row-main">
          <span class="setting-title">No sources yet</span>
          <span class="setting-meta">Add a source, then preview or sync its hosts.</span>
        </div>
      </div>
    </div>
  {:else}
    <div class="settings-group">
      {#each sources as source (source.id)}
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">
              {source.name}
              <span class="badge badge-neutral">{kindLabel(source.kind)}</span>
              {#if (source.stale ?? 0) > 0}
                <span class="badge badge-warning">{source.stale} stale</span>
              {/if}
            </span>
            <span class="setting-meta">
              {plural(source.hosts ?? 0, "host")} ·
              {source.last_sync_at ? `synced ${formatRelative(source.last_sync_at)}` : "never synced"}
            </span>
            {#if source.last_sync_error}
              <span class="text-xs text-error">Last sync failed: {source.last_sync_error}</span>
            {/if}
            {#if reports[source.name]}
              {@const report = reports[source.name]}
              {@const chips = reportChips(report)}
              <div class="mt-1 flex flex-wrap items-center gap-1.5 text-xs">
                <span class="text-muted">{report.dry_run ? "Preview (nothing written yet):" : "Synced:"}</span>
                {#each chips as chip (chip.label)}
                  <span class="badge {chip.cls}">{chip.label}</span>
                {:else}
                  <span class="text-muted">no changes</span>
                {/each}
                {#if report.dry_run && chips.length > 0}
                  <button
                    type="button"
                    class="btn btn-primary btn-sm"
                    disabled={busy === source.name}
                    onclick={() => syncSource(source, false)}
                  >
                    Apply
                  </button>
                {/if}
              </div>
              {#if report.renamed.length > 0}
                <span class="text-xs text-muted">
                  Name taken, added as: {report.renamed.map(([from, to]) => `${from} → ${to}`).join(", ")}
                </span>
              {/if}
            {/if}
          </div>
          <div class="setting-control">
            {#if source.kind === "team"}
              <!-- Team vaults need the device passphrase and are synced by the CLI. -->
              <span class="setting-meta">Synced with <code>bssh teams sync</code></span>
            {:else}
              <button
                type="button"
                class="btn btn-ghost"
                disabled={busy === source.name}
                onclick={() => syncSource(source, true)}
              >
                <Eye size={14} />
                Preview
              </button>
              <button
                type="button"
                class="btn btn-secondary"
                disabled={busy === source.name}
                onclick={() => syncSource(source, false)}
              >
                <RefreshCw size={14} />
                Sync
              </button>
            {/if}
            <button
              type="button"
              class="btn btn-ghost"
              onclick={() => {
                removeTarget = source;
                deleteHosts = false;
              }}
            >
              <Trash2 size={14} />
              Remove
            </button>
          </div>
        </div>

        {#if removeTarget?.id === source.id}
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Remove {source.name}?</span>
              <label class="setting-meta cursor-pointer">
                <input type="checkbox" bind:checked={deleteHosts} />
                Also delete the {plural(source.hosts ?? 0, "connection")} it owns (otherwise they stay as manual hosts)
              </label>
            </div>
            <div class="setting-control">
              <button type="button" class="btn btn-ghost" onclick={() => (removeTarget = null)}>
                Cancel
              </button>
              <button type="button" class="btn btn-danger" onclick={() => removeSource(source)}>
                Remove
              </button>
            </div>
          </div>
        {/if}
      {/each}
    </div>
  {/if}

  <p class="-mt-3 text-xs text-muted">
    Manual connections are never modified. Hosts missing from a sync are marked stale and only
    deleted by `bssh source sync --prune`.
  </p>
</div>
