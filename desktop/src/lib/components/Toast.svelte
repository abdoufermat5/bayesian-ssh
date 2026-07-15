<script lang="ts">
  import { CheckCircle2, AlertCircle, Info } from "lucide-svelte";
  import { getNotificationState } from "$lib/stores/notifications.svelte";

  const notification = getNotificationState();
</script>

{#if notification.visible}
  <div
    class="fixed bottom-5 right-5 bg-surface-raised border text-primary px-4 py-2.5 rounded-xl shadow-lg z-[1000] text-[13px] flex items-center gap-2 animate-[toast-enter_0.3s_cubic-bezier(0.16,1,0.3,1)_forwards]
      {notification.type === 'success' ? 'border-success/30' : notification.type === 'error' ? 'border-danger/30' : 'border-border'}"
  >
    {#if notification.type === "success"}
      <CheckCircle2 size={16} class="text-success shrink-0" />
    {:else if notification.type === "error"}
      <AlertCircle size={16} class="text-danger shrink-0" />
    {:else}
      <Info size={16} class="text-accent shrink-0" />
    {/if}
    <span>{notification.text}</span>
  </div>
{/if}
