<script lang="ts">
  import { TerminalSquare, AlertTriangle, Minimize2, Power } from "lucide-svelte";

  interface Props {
    activeCount: number;
    onCancel: () => void;
    onMinimize: () => void;
    onQuit: () => void;
  }

  let { activeCount, onCancel, onMinimize, onQuit }: Props = $props();
</script>

<div
  class="fixed inset-0 bg-black/75 backdrop-blur-sm flex items-center justify-center z-[100]"
  onclick={onCancel}
  role="presentation"
>
  <div
    class="bg-surface border border-amber-500/25 rounded-2xl w-[420px] flex flex-col items-center p-7 shadow-2xl text-center animate-[modal-enter_0.2s_cubic-bezier(0.16,1,0.3,1)_forwards] [box-shadow:0_0_0_1px_rgba(245,158,11,0.08),var(--shadow-xl)]"
    onclick={(e) => e.stopPropagation()}
    role="presentation"
  >
    <div class="w-14 h-14 rounded-full bg-amber-500/10 border border-amber-500/20 flex items-center justify-center text-amber-400 mb-4">
      <AlertTriangle size={28} />
    </div>

    <div class="flex flex-col items-center gap-1 mb-6">
      <h3 class="text-base font-semibold text-primary m-0">Active Connections Running</h3>
      <p class="text-sm font-semibold text-amber-400 mt-1">
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
          class="flex-1 py-2.5 rounded-lg border border-border bg-surface-input/60 text-secondary text-[13px] font-medium cursor-pointer transition-all duration-100 hover:bg-white/[0.05] hover:text-primary hover:border-border-hover flex items-center justify-center gap-1.5"
          onclick={onMinimize}
        >
          <Minimize2 size={14} />
          Minimize to Tray
        </button>
        <button
          type="button"
          class="flex-1 py-2.5 rounded-lg border border-danger/35 bg-danger/10 text-red-400 text-[13px] font-semibold cursor-pointer flex items-center justify-center gap-1.5 transition-all duration-100 hover:bg-danger/[0.18] hover:text-red-300 hover:border-danger/50"
          onclick={onQuit}
        >
          <Power size={14} />
          Disconnect & Quit
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
  </div>
</div>
