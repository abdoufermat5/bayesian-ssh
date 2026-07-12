<script lang="ts">
  import { getNotificationState } from "$lib/stores/notifications.svelte";

  const notification = getNotificationState();
</script>

{#if notification.visible}
  <div
    class="toast-notification"
    class:success={notification.type === "success"}
    class:error={notification.type === "error"}
  >
    <span>{notification.text}</span>
  </div>
{/if}
