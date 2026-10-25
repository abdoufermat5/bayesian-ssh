<script lang="ts">
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { PendingChangePrompt } from "$lib/stores/terminal.svelte";

  interface Props {
    prompt: PendingChangePrompt;
    /** Accepts the new route and retries the connection. */
    onConfirm: () => void;
    /** Keeps the change unconfirmed. */
    onCancel: () => void;
  }

  let { prompt, onConfirm, onCancel }: Props = $props();
</script>

<ModalShell open={true} title="Confirm connection change" onClose={onCancel} width="sm">
  <div class="modal-header">
    <h2 class="modal-title break-words">Confirm connection change</h2>
    <button type="button" class="modal-close" onclick={onCancel} aria-label="Close">
      <X size={16} />
    </button>
  </div>

  <div class="modal-body flex flex-col gap-2.5">
    <p class="m-0 text-sm leading-relaxed text-secondary">
      A team sync changed <span class="font-medium text-primary">{prompt.connection.name}</span>. Connecting
      sends your credentials and Kerberos ticket to the new values below.
    </p>
    <div class="code-block max-h-40 whitespace-pre-wrap break-all">{prompt.changes.join("\n")}</div>
    <p class="m-0 text-xs leading-relaxed text-muted">
      Agent forwarding is off for this connection until you confirm.
    </p>
  </div>

  <div class="modal-footer">
    <button type="button" class="btn btn-secondary" onclick={onCancel} data-autofocus>Cancel</button>
    <button type="button" class="btn btn-primary" onclick={onConfirm}>Confirm and connect</button>
  </div>
</ModalShell>
