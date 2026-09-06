<script lang="ts">
  import { TerminalSquare, AlertTriangle, Minimize2, Power } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    activeCount: number;
    onCancel: () => void;
    onMinimize: () => void;
    onQuit: () => void;
  }

  let { activeCount, onCancel, onMinimize, onQuit }: Props = $props();
</script>

<ModalShell
  open={true}
  title="Active Connections Running"
  onClose={onCancel}
  width="sm"
  panelClass="items-center p-7 text-center"
  panelStyle="border-color: color-mix(in srgb, var(--color-warning) 25%, transparent); box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-warning) 8%, transparent), var(--shadow-xl);"
>
  <div class="w-14 h-14 rounded-full bg-warning/10 border border-warning/20 flex items-center justify-center text-warning mb-4">
      <AlertTriangle size={28} />
    </div>

    <div class="flex flex-col items-center gap-1 mb-6">
      <h3 class="text-base font-semibold text-primary m-0">Active Connections Running</h3>
      <p class="text-sm font-semibold text-warning mt-1">
        {activeCount} active SSH connection{activeCount === 1 ? "" : "s"} open
      </p>
      <p class="text-xs text-muted mt-1 leading-relaxed px-2">
        Quitting will disconnect all active remote sessions. You can minimize to the tray to keep your terminal sessions running in the background.
      </p>
    </div>

    <div class="flex flex-col gap-2 w-full">
      <div class="flex gap-2 w-full">
        <button
          type="button"
          class="btn btn-secondary flex-1 py-2 text-xs"
          onclick={onMinimize}
        >
          <Minimize2 size={13} />
          <span>Minimize to Tray</span>
        </button>
        <button
          type="button"
          class="btn btn-danger flex-1 py-2 text-xs"
          onclick={onQuit}
        >
          <Power size={13} />
          <span>Disconnect & Quit</span>
        </button>
      </div>

      <button
        type="button"
        class="w-full py-2 rounded-lg border-none bg-transparent text-muted text-xs cursor-pointer transition-colors hover:text-primary mt-0.5"
        onclick={onCancel}
      >
        Cancel
      </button>
    </div>
</ModalShell>
