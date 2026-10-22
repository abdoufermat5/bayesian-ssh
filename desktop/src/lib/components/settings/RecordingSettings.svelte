<script lang="ts">
  import type { DesktopSettings } from "$lib/types";

  interface Props {
    settings: DesktopSettings;
    onSave: () => void;
  }

  let { settings = $bindable(), onSave }: Props = $props();

  let tagsInput = $state((settings.record_tags ?? []).join(", "));

  $effect(() => {
    tagsInput = (settings.record_tags ?? []).join(", ");
  });

  function commitTags() {
    settings.record_tags = tagsInput
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    onSave();
  }
</script>

<div class="settings-page">
  <section>
    <h2 class="settings-group-title">Session recording</h2>
    <div class="settings-group">
      <label class="setting-row cursor-pointer">
        <span class="setting-row-main">
          <span class="setting-title">Record sessions</span>
          <span class="setting-meta">
            Save what the remote prints so you can search and replay it from History. Keystrokes are
            never recorded; output can still contain secrets, so files are private to your user.
          </span>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={Boolean(settings.record_sessions)}
          onchange={(e) => {
            settings.record_sessions = (e.target as HTMLInputElement).checked;
            onSave();
          }}
        />
      </label>
      <div class="setting-row" class:opacity-50={!settings.record_sessions}>
        <div class="setting-row-main">
          <label class="setting-title" for="settings-record-tags">Only for tags</label>
          <span class="setting-meta">Comma-separated tags to record; leave empty to record all hosts.</span>
        </div>
        <div class="setting-control">
          <input
            id="settings-record-tags"
            type="text"
            placeholder="prod, critical"
            bind:value={tagsInput}
            onchange={commitTags}
            disabled={!settings.record_sessions}
            spellcheck="false"
            autocomplete="off"
            class="input w-56"
          />
        </div>
      </div>
    </div>
  </section>

  <p class="-mt-3 type-meta">Changes apply immediately.</p>
</div>
