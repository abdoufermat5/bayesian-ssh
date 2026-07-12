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
  let default_user = $state(defaultUser);
  let default_port = $state(22);
  let ssh_config_path = $state(defaultSshConfigPath);
  let theme = $state("zinc");
  let auto_start_agent = $state(false);
  let import_ssh_config = $state(true);
  let fuzzy_search = $state(false);

  $effect(() => {
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

<div class="onboarding-backdrop" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
  <div class="onboarding-dialog">
    <div class="onboarding-header">
      <div class="onboarding-brand">
        <TerminalSquare size={22} />
        <div>
          <h2 id="onboarding-title">Welcome to Bayesian SSH</h2>
          <p>Let's set up your workspace in a minute.</p>
        </div>
      </div>
      <div class="onboarding-steps">
        {#each steps as label, index}
          <div class="onboarding-step" class:active={index === step} class:done={index < step}>
            <span class="step-dot">{index + 1}</span>
            <span class="step-label">{label}</span>
          </div>
        {/each}
      </div>
    </div>

    <div class="onboarding-body">
      {#if step === 0}
        <div class="onboarding-panel">
          <Sparkles size={40} class="panel-icon" />
          <h3>Smart SSH, right on your desktop</h3>
          <p>
            Bayesian SSH ranks your hosts by how often and how recently you connect, so the servers
            you need are always one keystroke away.
          </p>
          <ul>
            <li>Separate workspaces (profiles) for work, personal, and staging</li>
            <li>Built-in terminals with live session persistence</li>
            <li>Import hosts from your existing OpenSSH config</li>
          </ul>
        </div>
      {:else if step === 1}
        <div class="onboarding-panel onboarding-form">
          <h3>Choose your workspace</h3>
          <p class="panel-hint">
            A workspace stores your hosts, history, and settings. Data lives at:
            <code>{configRoot}</code>
          </p>

          <label class="choice-card" class:selected={!useCustomProfile}>
            <input type="radio" bind:group={useCustomProfile} value={false} />
            <div>
              <strong>Default workspace</strong>
              <span>Recommended for most users. Uses the built-in <code>default</code> profile.</span>
            </div>
          </label>

          <label class="choice-card" class:selected={useCustomProfile}>
            <input type="radio" bind:group={useCustomProfile} value={true} />
            <div>
              <strong>Custom workspace profile</strong>
              <span>Create a named profile (e.g. <code>work</code>, <code>homelab</code>).</span>
            </div>
          </label>

          {#if useCustomProfile}
            <div class="form-group">
              <label for="onboard-profile">Profile name</label>
              <input
                id="onboard-profile"
                type="text"
                class="cyber-input"
                placeholder="e.g. work"
                bind:value={profileName}
              />
            </div>
          {/if}
        </div>
      {:else if step === 2}
        <div class="onboarding-panel onboarding-form">
          <h3>SSH defaults</h3>
          <p class="panel-hint">Used when adding new hosts. You can change these later in Settings.</p>

          <div class="form-row">
            <div class="form-group">
              <label for="onboard-user">Default SSH user</label>
              <input id="onboard-user" type="text" class="cyber-input" bind:value={default_user} />
            </div>
            <div class="form-group">
              <label for="onboard-port">Default port</label>
              <input id="onboard-port" type="number" class="cyber-input" bind:value={default_port} />
            </div>
          </div>

          <div class="form-group">
            <label for="onboard-ssh-config">OpenSSH config path</label>
            <div class="input-with-action">
              <input
                id="onboard-ssh-config"
                type="text"
                class="cyber-input"
                placeholder="~/.ssh/config"
                bind:value={ssh_config_path}
              />
              <button type="button" class="browse-btn" onclick={pickSshConfig}>
                <FolderOpen size={14} />
                Browse
              </button>
            </div>
          </div>

          <label class="checkbox-row">
            <input type="checkbox" bind:checked={import_ssh_config} />
            Import hosts from OpenSSH config now
          </label>
        </div>
      {:else}
        <div class="onboarding-panel onboarding-form">
          <h3>Almost done</h3>
          <p class="panel-hint">Pick a look and optional agent behavior for your first session.</p>

          <div class="form-group">
            <label for="onboard-theme">Theme</label>
            <select id="onboard-theme" class="cyber-select" bind:value={theme}>
              <option value="zinc">Slate Minimalist (Zinc)</option>
              <option value="cyberpunk">Cyberpunk Neon</option>
              <option value="oled">OLED Pitch Black</option>
              <option value="slate">Sleek Navy (Slate)</option>
            </select>
          </div>

          <label class="checkbox-row">
            <input type="checkbox" bind:checked={auto_start_agent} />
            Auto-start SSH agent on launch
          </label>

          <label class="checkbox-row">
            <input type="checkbox" bind:checked={fuzzy_search} />
            Prefer fuzzy search over Bayesian ranking
          </label>
        </div>
      {/if}
    </div>

    <div class="onboarding-footer">
      <button class="cancel-btn" onclick={back} disabled={step === 0 || busy}>
        <ChevronLeft size={14} />
        Back
      </button>
      <button class="save-btn onboarding-next" onclick={next} disabled={busy}>
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

<style>
  .onboarding-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.82);
    backdrop-filter: blur(8px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
    padding: 24px;
  }

  .onboarding-dialog {
    width: min(680px, 100%);
    max-height: min(88vh, 760px);
    background: var(--bg-app);
    border: 1px solid var(--border-color);
    border-radius: 16px;
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.65);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .onboarding-header {
    padding: 24px 28px 16px;
    border-bottom: 1px solid var(--border-color);
  }

  .onboarding-brand {
    display: flex;
    gap: 14px;
    align-items: flex-start;
    color: var(--accent-cyan);
    margin-bottom: 18px;
  }

  .onboarding-brand h2 {
    margin: 0;
    font-size: 1.2rem;
    color: var(--text-primary);
  }

  .onboarding-brand p {
    margin: 4px 0 0;
    font-size: 0.85rem;
    color: var(--text-muted);
  }

  .onboarding-steps {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }

  .onboarding-step {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .onboarding-step.active,
  .onboarding-step.done {
    color: var(--text-secondary);
  }

  .step-dot {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 1px solid var(--border-color);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 0.68rem;
    font-weight: 700;
    flex-shrink: 0;
  }

  .onboarding-step.active .step-dot {
    border-color: rgba(0, 240, 255, 0.45);
    color: var(--accent-cyan);
    background: rgba(0, 240, 255, 0.08);
  }

  .onboarding-step.done .step-dot {
    background: rgba(16, 185, 129, 0.12);
    border-color: rgba(16, 185, 129, 0.35);
    color: var(--green-emerald);
  }

  .step-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .onboarding-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 24px 28px;
  }

  .onboarding-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .onboarding-panel :global(.panel-icon) {
    color: var(--accent-cyan);
    margin-bottom: 4px;
  }

  .onboarding-panel h3 {
    margin: 0;
    font-size: 1.05rem;
    color: var(--text-primary);
  }

  .onboarding-panel p,
  .panel-hint {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.88rem;
    line-height: 1.5;
  }

  .onboarding-panel ul {
    margin: 8px 0 0;
    padding-left: 1.1rem;
    color: var(--text-secondary);
    font-size: 0.85rem;
    line-height: 1.6;
  }

  .onboarding-panel code,
  .panel-hint code {
    font-family: "JetBrains Mono", monospace;
    font-size: 0.78rem;
    color: var(--accent-cyan);
  }

  .onboarding-form {
    gap: 16px;
  }

  .choice-card {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 14px 16px;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.02);
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
  }

  .choice-card.selected {
    border-color: rgba(0, 240, 255, 0.35);
    background: rgba(0, 240, 255, 0.05);
  }

  .choice-card strong {
    display: block;
    color: var(--text-primary);
    font-size: 0.9rem;
    margin-bottom: 4px;
  }

  .choice-card span {
    color: var(--text-muted);
    font-size: 0.8rem;
    line-height: 1.4;
  }

  .onboarding-footer {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 28px 22px;
    border-top: 1px solid var(--border-color);
  }

  .onboarding-next {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
</style>
