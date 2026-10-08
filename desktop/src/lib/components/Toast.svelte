<script lang="ts">
  import { CheckCircle2, AlertCircle, Info, X } from "lucide-svelte";
  import { getToasts, dismissNotification } from "$lib/stores/notifications.svelte";
</script>

<div
  class="pointer-events-none fixed bottom-4 right-4 z-[400] flex flex-col items-end gap-2"
  aria-live="polite"
>
  {#each getToasts() as toast (toast.id)}
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
        class="btn-icon btn-icon-sm -mr-1 -mt-0.5"
      >
        <X size={14} />
      </button>
    </div>
  {/each}
</div>
