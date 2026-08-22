<script lang="ts">
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

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

<ModalShell
  open={true}
  title={title}
  onClose={onCancel}
  width="sm"
  panelClass="items-center p-8 text-center"
  panelStyle="border-color: color-mix(in srgb, var(--color-error) 25%, transparent); box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-error) 6%, transparent), var(--shadow-xl);"
>
  <div class="w-14 h-14 rounded-full bg-error/8 border border-error/20 flex items-center justify-center text-error mb-4">
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
      <p class="text-sm font-semibold text-error mt-1 break-all">{label}</p>
      <p class="text-[11px] font-mono text-muted mt-0.5">{subtitle}</p>
      <p class="text-[11px] text-error mt-2 px-3 py-1.25 rounded-full bg-error/6 border border-error/12">{warning}</p>
    </div>

    <div class="flex gap-2 w-full">
      <button
        class="btn btn-secondary flex-1 py-2.5 text-sm"
        onclick={onCancel}
      >
        Cancel
      </button>
      <button
        class="btn btn-danger flex-1 py-2.5 text-sm"
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
</ModalShell>
