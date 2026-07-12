import type { NotificationType } from "$lib/types";

let text = $state("");
let type = $state<NotificationType>("info");
let visible = $state(false);
let timeoutId: ReturnType<typeof setTimeout> | null = null;

export function notify(message: string, notificationType: NotificationType = "info") {
  text = message;
  type = notificationType;
  visible = true;

  if (timeoutId) clearTimeout(timeoutId);
  timeoutId = setTimeout(() => {
    visible = false;
  }, 3000);
}

export function getNotificationState() {
  return {
    get text() {
      return text;
    },
    get type() {
      return type;
    },
    get visible() {
      return visible;
    },
  };
}
