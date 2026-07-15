<script lang="ts">
  import { ChevronLeft, ChevronRight, FolderOpen, Sparkles, TerminalSquare } from "lucide-svelte";
  import type { OnboardingPayload } from "$lib/types";

  interface Props {
    defaultUser: string;
    defaultSshConfigPath: string;
    configRoot: string;
    onBrowseSshConfig: () => Promise<string | null>;
    onComplete: (payload: OnboardingPayload) => Promise<void>;
  }

  let { defaultUser, defaultSshConfigPath, configRoot, onBrowseSshConfig, onComplete }: Props = $props();

  const steps = ["Welcome", "Workspace", "SSH Defaults", "Finish"];
  let step = $state(0);
  let busy = $state(false);

  let useCustomProfile = $state(false);
  let profileName = $state("default");
  let default_user = $state("");
  let default_port = $state(22);
  let ssh_config_path = $state("");
  let theme = $state("zinc");
  let auto_start_agent = $state(false);
  let import_ssh_config = $state(true);
  let fuzzy_search = $state(false);

  $effect(() => {
    if (defaultUser && !default_user) {
      default_user = defaultUser;
    }
    if (defaultSshConfigPath && !ssh_config_path) {
      ssh_config_path = defaultSshConfigPath;
    }
  });

  async function next() {
    if (step === 1 && useCustomProfile && !profileName.trim()) return;
    if (step === 2 && !default_user.trim()) return;
    if (step < steps.length - 1) {
      step += 1;
      return;
    }
    await finish();
  }

  function back() {
    if (step > 0) step -= 1;
  }

  async function finish() {
    busy = true;
    try {
      const name = useCustomProfile ? profileName.trim() : "default";
      await onComplete({
        profile_name: name,
        create_profile: useCustomProfile && name !== "default",
        default_user: default_user.trim(),
        default_port: default_port,
        ssh_config_path: ssh_config_path.trim() || null,
        theme,
        auto_start_agent,
        import_ssh_config,
        fuzzy_search,
      });
    } finally {
      busy = false;
    }
  }

  async function pickSshConfig() {
    const selected = await onBrowseSshConfig();
    if (selected) ssh_config_path = selected;
  }
</script>

<div
  class="fixed inset-0 bg-black/80 backdrop-blur-md flex items-center justify-center z-[200] p-6"
  role="dialog"
  aria-modal="true"
  aria-labelledby="onboarding-title"
>
  <div class="w-[680px] max-w-full max-h-[88vh] bg-surface border border-border rounded-2xl shadow-xl flex flex-col overflow-hidden">
    <div class="p-6 pb-4 border-b border-border flex-shrink-0">
      <div class="flex gap-3.5 items-start text-accent mb-4">
        <TerminalSquare size={22} class="mt-0.5 shrink-0" />
        <div>
          <h2 id="onboarding-title" class="m-0 text-base font-bold text-primary">Welcome to Bayesian SSH</h2>
          <p class="m-0 text-[11px] text-muted mt-0.5">Let's set up your workspace in a minute.</p>
        </div>
      </div>
      <div class="grid grid-cols-4 gap-2">
        {#each steps as label, index}
          <div
            class="flex items-center gap-2 text-[11px] text-muted
              {index === step || index < step ? '!text-secondary' : ''}"
          >
            <span
              class="w-[22px] h-[22px] rounded-full border border-border inline-flex items-center justify-center text-[10px] font-bold shrink-0
                {index === step ? 'border-accent/45 text-accent bg-accent/8' : ''}
                {index < step ? 'bg-success/12 border-success/35 text-success' : ''}"
            >
              {index + 1}
            </span>
            <span class="overflow-hidden text-ellipsis whitespace-nowrap">{label}</span>
          </div>
        {/each}
      </div>
    </div>

    <div class="flex-1 min-h-0 overflow-y-auto px-6 py-5">
      {#if step === 0}
        <div class="flex flex-col gap-3">
          <Sparkles size={40} class="text-accent mb-1" />
          <h3 class="m-0 text-base font-semibold text-primary">Smart SSH, right on your desktop</h3>
          <p class="m-0 text-secondary text-sm leading-relaxed">
            Bayesian SSH ranks your hosts by how often and how recently you connect, so the servers
            you need are always one keystroke away.
          </p>
          <ul class="mt-2 pl-4 text-secondary text-[13px] leading-relaxed list-disc">
            <li>Separate workspaces (profiles) for work, personal, and staging</li>
            <li>Built-in terminals with live session persistence</li>
            <li>Import hosts from your existing OpenSSH config</li>
          </ul>
        </div>
      {:else if step === 1}
        <div class="flex flex-col gap-4">
          <h3 class="m-0 text-base font-semibold text-primary">Choose your workspace</h3>
          <p class="m-0 text-secondary text-sm leading-relaxed">
            A workspace stores your hosts, history, and settings. Data lives at:
            <code class="font-mono text-[11px] text-accent bg-white/4 p-0.5 px-1 rounded">{configRoot}</code>
          </p>

          <label
            class="flex gap-3 items-start p-3.5 px-4 border rounded-xl cursor-pointer transition-colors duration-150 bg-white/[0.02]
              {!useCustomProfile ? 'border-accent/35 bg-accent/5' : 'border-border'}"
          >
            <input type="radio" bind:group={useCustomProfile} value={false} class="mt-1 accent-accent" />
            <div>
              <strong class="block text-primary text-xs font-semibold mb-1">Default workspace</strong>
              <span class="text-secondary text-[11px] leading-normal">Recommended for most users. Uses the built-in <code class="font-mono text-[11px] text-accent">default</code> profile.</span>
            </div>
          </label>

          <label
            class="flex gap-3 items-start p-3.5 px-4 border rounded-xl cursor-pointer transition-colors duration-150 bg-white/[0.02]
              {useCustomProfile ? 'border-accent/35 bg-accent/5' : 'border-border'}"
          >
            <input type="radio" bind:group={useCustomProfile} value={true} class="mt-1 accent-accent" />
            <div>
              <strong class="block text-primary text-xs font-semibold mb-1">Custom workspace profile</strong>
              <span class="text-secondary text-[11px] leading-normal">Create a named profile (e.g. <code class="font-mono text-[11px] text-accent">work</code>, <code class="font-mono text-[11px] text-accent">homelab</code>).</span>
            </div>
          </label>

          {#if useCustomProfile}
            <div class="flex flex-col gap-1.5 mt-1">
              <label for="onboard-profile" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Profile name</label>
              <input
                id="onboard-profile"
                type="text"
                class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
                placeholder="e.g. work"
                bind:value={profileName}
              />
            </div>
          {/if}
        </div>
      {:else if step === 2}
        <div class="flex flex-col gap-4">
          <h3 class="m-0 text-base font-semibold text-primary">SSH defaults</h3>
          <p class="m-0 text-secondary text-sm leading-relaxed">Used when adding new hosts. You can change these later in Settings.</p>

          <div class="flex gap-3">
            <div class="flex flex-col gap-1.5 flex-1">
              <label for="onboard-user" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Default SSH user</label>
              <input
                id="onboard-user"
                type="text"
                class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
                bind:value={default_user}
              />
            </div>
            <div class="flex flex-col gap-1.5 flex-1">
              <label for="onboard-port" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Default port</label>
              <input
                id="onboard-port"
                type="number"
                class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
                bind:value={default_port}
              />
            </div>
          </div>

          <div class="flex flex-col gap-1.5">
            <label for="onboard-ssh-config" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">OpenSSH config path</label>
            <div class="flex gap-2">
              <input
                id="onboard-ssh-config"
                type="text"
                class="flex-1 bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
                placeholder="~/.ssh/config"
                bind:value={ssh_config_path}
              />
              <button
                type="button"
                class="bg-white/[0.04] border border-border text-secondary px-3.5 rounded-lg cursor-pointer font-semibold flex items-center gap-1.5 text-xs whitespace-nowrap transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.06]"
                onclick={pickSshConfig}
              >
                <FolderOpen size={14} />
                Browse
              </button>
            </div>
          </div>

          <label class="flex items-center gap-2 text-[13px] cursor-pointer select-none text-secondary py-1">
            <input type="checkbox" bind:checked={import_ssh_config} class="cursor-pointer accent-accent w-[16px] h-[16px]" />
            <span>Import hosts from OpenSSH config now</span>
          </label>
        </div>
      {:else}
        <div class="flex flex-col gap-4">
          <h3 class="m-0 text-base font-semibold text-primary">Almost done</h3>
          <p class="m-0 text-secondary text-sm leading-relaxed">Pick a look and optional agent behavior for your first session.</p>

          <div class="flex flex-col gap-1.5">
            <label for="onboard-theme" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Theme</label>
            <select
              id="onboard-theme"
              class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] cursor-pointer transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)] w-full"
              bind:value={theme}
            >
              <option value="zinc">Slate Minimalist (Zinc)</option>
              <option value="cyberpunk">Cyberpunk Neon</option>
              <option value="oled">OLED Pitch Black</option>
              <option value="slate">Sleek Navy (Slate)</option>
            </select>
          </div>

          <label class="flex items-center gap-2 text-[13px] cursor-pointer select-none text-secondary py-1">
            <input type="checkbox" bind:checked={auto_start_agent} class="cursor-pointer accent-accent w-[16px] h-[16px]" />
            <span>Auto-start SSH agent on launch</span>
          </label>

          <label class="flex items-center gap-2 text-[13px] cursor-pointer select-none text-secondary py-1">
            <input type="checkbox" bind:checked={fuzzy_search} class="cursor-pointer accent-accent w-[16px] h-[16px]" />
            <span>Prefer fuzzy search over Bayesian ranking</span>
          </label>
        </div>
      {/if}
    </div>

    <div class="flex justify-between gap-3 px-6 py-4 border-t border-border shrink-0">
      <button
        class="py-2 px-4 rounded-lg text-[13px] font-semibold cursor-pointer bg-transparent border border-border text-secondary flex items-center gap-1.5 transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.03] disabled:opacity-50 disabled:cursor-not-allowed"
        onclick={back}
        disabled={step === 0 || busy}
      >
        <ChevronLeft size={14} />
        Back
      </button>
      <button
        class="py-2 px-4 rounded-lg text-[13px] font-semibold cursor-pointer bg-accent border-none text-white inline-flex items-center gap-1.5 transition-colors duration-100 hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
        onclick={next}
        disabled={busy}
      >
        {#if step === steps.length - 1}
          {busy ? "Setting up..." : "Get started"}
        {:else}
          Continue
          <ChevronRight size={14} />
        {/if}
      </button>
    </div>
  </div>
</div>
