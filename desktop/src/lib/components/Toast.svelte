<script lang="ts">
  import { CheckCircle2, AlertCircle, Info, X } from "lucide-svelte";
  import { getNotificationState, dismissNotification } from "$lib/stores/notifications.svelte";

  const notification = getNotificationState();
</script>

{#if notification.visible}
  <div
    class="toast {notification.type === 'success'
      ? 'toast-success'
      : notification.type === 'error'
        ? 'toast-error'
        : 'toast-info'}"
    role={notification.type === "error" ? "alert" : "status"}
    aria-live={notification.type === "error" ? "assertive" : "polite"}
  >
    {#if notification.type === "success"}
      <CheckCircle2 size={16} class="text-success shrink-0" />
    {:else if notification.type === "error"}
      <AlertCircle size={16} class="text-error shrink-0" />
    {:else}
      <Info size={16} class="text-accent shrink-0" />
    {/if}
    <span>{notification.text}</span>
    <button
      type="button"
      onclick={dismissNotification}
      title="Dismiss notification"
      aria-label="Dismiss notification"
      class="btn-icon shrink-0"
    >
      <X size={14} />
    </button>
  </div>
{/if}
