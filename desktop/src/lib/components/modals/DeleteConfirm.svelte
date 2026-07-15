<script lang="ts">
  interface Props {
    title?: string;
    confirmLabel?: string;
    label: string;
    subtitle: string;
    warning?: string;
    onCancel: () => void;
    onConfirm: () => void;
  }

  let {
    title = "Delete Connection",
    confirmLabel = "Delete",
    label,
    subtitle,
    warning = "This action cannot be undone.",
    onCancel,
    onConfirm,
  }: Props = $props();
</script>

<div
  class="fixed inset-0 bg-black/75 backdrop-blur-sm flex items-center justify-center z-[100]"
  onclick={onCancel}
  role="presentation"
>
  <div
    class="bg-surface border border-danger/25 rounded-2xl w-[380px] flex flex-col items-center p-8 shadow-xl text-center animate-[modal-enter_0.2s_cubic-bezier(0.16,1,0.3,1)_forwards] [box-shadow:0_0_0_1px_rgba(239,68,68,0.06),var(--shadow-xl)]"
    onclick={(e) => e.stopPropagation()}
    role="presentation"
  >
    <div class="w-14 h-14 rounded-full bg-danger/8 border border-danger/20 flex items-center justify-center text-danger mb-4">
      <svg
        width="28"
        height="28"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <polyline points="3 6 5 6 21 6" />
        <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
        <path d="M10 11v6" />
        <path d="M14 11v6" />
        <path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
      </svg>
    </div>

    <div class="flex flex-col items-center gap-1 mb-6">
      <h3 class="text-base font-semibold text-primary m-0">{title}</h3>
      <p class="text-sm font-semibold text-danger mt-1 break-all">{label}</p>
      <p class="text-[11px] font-mono text-muted mt-0.5">{subtitle}</p>
      <p class="text-[11px] text-red-400 mt-2 px-3 py-1.25 rounded-full bg-danger/6 border border-danger/12">{warning}</p>
    </div>

    <div class="flex gap-2 w-full">
      <button
        class="flex-1 py-2.5 rounded-lg border border-border bg-transparent text-secondary text-[13px] font-medium cursor-pointer transition-all duration-100 hover:bg-white/[0.03] hover:text-primary hover:border-border-hover"
        onclick={onCancel}
      >
        Cancel
      </button>
      <button
        class="flex-1 py-2.5 rounded-lg border border-danger/35 bg-danger/10 text-red-400 text-[13px] font-semibold cursor-pointer flex items-center justify-center gap-1.5 transition-all duration-100 hover:bg-danger/[0.18] hover:text-red-300 hover:border-danger/50"
        onclick={onConfirm}
      >
        <svg
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polyline points="3 6 5 6 21 6" />
          <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
          <path d="M10 11v6" /><path d="M14 11v6" />
        </svg>
        {confirmLabel}
      </button>
    </div>
  </div>
</div>
