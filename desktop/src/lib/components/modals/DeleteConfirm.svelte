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

<div class="modal-backdrop" onclick={onCancel} role="dialog" aria-modal="true">
  <div class="delete-confirm-dialog" onclick={(e) => e.stopPropagation()}>
    <div class="delete-icon-ring">
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

    <div class="delete-confirm-body">
      <h3>{title}</h3>
      <p class="delete-confirm-name">{label}</p>
      <p class="delete-confirm-subtitle">{subtitle}</p>
      <p class="delete-confirm-warning">{warning}</p>
    </div>

    <div class="delete-confirm-actions">
      <button class="delete-cancel-btn" onclick={onCancel}>Cancel</button>
      <button class="delete-confirm-btn" onclick={onConfirm}>
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
