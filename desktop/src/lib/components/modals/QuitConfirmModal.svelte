<script lang="ts">
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    activeCount: number;
    onCancel: () => void;
    onMinimize: () => void;
    onQuit: () => void;
  }

  let { activeCount, onCancel, onMinimize, onQuit }: Props = $props();

  const sessions = $derived(`${activeCount} session${activeCount === 1 ? "" : "s"}`);
</script>

<ModalShell open={true} title="Quit Bayesian SSH" onClose={onCancel} width="md">
  <div class="modal-header">
    <h2 class="modal-title">Quit Bayesian SSH?</h2>
    <button type="button" class="modal-close" onclick={onCancel} aria-label="Close">
      <X size={16} />
    </button>
  </div>

  <div class="modal-body">
    <p class="m-0 text-sm leading-relaxed text-secondary">
      <span class="font-medium text-primary">{sessions}</span>
      {activeCount === 1 ? "is" : "are"} still open. Quitting disconnects
      {activeCount === 1 ? "it" : "them"}. Minimize to the tray to keep
      {activeCount === 1 ? "it" : "them"} running in the background.
    </p>
  </div>

  <div class="modal-footer">
    <button type="button" class="btn btn-ghost mr-auto" onclick={onCancel}>Cancel</button>
    <button type="button" class="btn btn-secondary" onclick={onMinimize} data-autofocus>Minimize to tray</button>
    <button type="button" class="btn btn-danger" onclick={onQuit}>Disconnect and quit</button>
  </div>
</ModalShell>
