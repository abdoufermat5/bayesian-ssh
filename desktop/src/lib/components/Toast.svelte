<script lang="ts">
  import { CheckCircle2, AlertCircle, Info, X } from "lucide-svelte";
  import { getToasts, dismissNotification } from "$lib/stores/notifications.svelte";

  const MAX_DISPLAYED = 3;
  const allToasts = $derived(getToasts());
  const visibleToasts = $derived(allToasts.slice(-MAX_DISPLAYED));
  const hiddenCount = $derived(Math.max(0, allToasts.length - visibleToasts.length));
</script>

<div
  class="pointer-events-none fixed right-4 top-12 z-[400] flex flex-col items-end gap-2"
  aria-live="polite"
>
  {#each visibleToasts as toast (toast.id)}
    <div class="toast" role={toast.type === "error" ? "alert" : "status"}>
      <span class="mt-px flex shrink-0">
        {#if toast.type === "success"}
          <CheckCircle2 size={16} class="text-success" />
        {:else if toast.type === "error"}
          <AlertCircle size={16} class="text-error" />
        {:else}
          <Info size={16} class="text-accent" />
        {/if}
      </span>
      <span class="min-w-0 flex-1 break-words leading-snug">{toast.text}</span>
      <button
        type="button"
        onclick={() => dismissNotification(toast.id)}
        aria-label="Dismiss notification"
        title="Dismiss"
        class="btn-icon btn-icon-sm -mr-1 -mt-0.5"
      >
        <X size={14} />
      </button>
    </div>
  {/each}

  {#if hiddenCount > 0}
    <button
      type="button"
      onclick={() => dismissNotification()}
      class="pointer-events-auto cursor-pointer rounded-md border border-border-floating bg-surface-raised px-2.5 py-1 text-xs text-secondary shadow-md transition-colors duration-fast hover:bg-hover-on-floating hover:text-primary"
    >
      +{hiddenCount} more · Dismiss all
    </button>
  {/if}
</div>
