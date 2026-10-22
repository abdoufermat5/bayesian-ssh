<script lang="ts">
  import { Check, Copy } from "lucide-svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";
  import { notify } from "$lib/stores/notifications.svelte";
  import { chunkFingerprint } from "./teams";

  let {
    value,
    copyable = true,
    label = "fingerprint",
  }: {
    value: string;
    copyable?: boolean;
    /** Accessible name suffix for the copy button, e.g. "your fingerprint". */
    label?: string;
  } = $props();

  const parts = $derived(chunkFingerprint(value));
  let copied = $state(false);

  function copy() {
    copyTextWithFallback(value);
    copied = true;
    notify("Copied", "success");
    setTimeout(() => (copied = false), 2000);
  }
</script>

<!-- Chunks are separate spans spaced by margin, so a selection copies the
     fingerprint without spaces; never truncated, wraps on narrow widths. -->
<span class="inline-flex min-w-0 flex-wrap items-center gap-2">
  <code class="fingerprint select-all font-mono type-body leading-relaxed text-primary">
    <span class="text-muted">{parts.prefix}</span>{#each parts.chunks as chunk, i (i)}<span class="chunk">{chunk}</span>{/each}
  </code>
  {#if copyable}
    <button type="button" class="btn btn-ghost btn-sm" onclick={copy} aria-label="Copy {label}">
      {#if copied}<Check size={14} />{:else}<Copy size={14} />{/if}
      Copy
    </button>
  {/if}
</span>

<style>
  .fingerprint {
    word-break: break-all;
  }
  .chunk {
    margin-left: 0.5ch;
  }
</style>
