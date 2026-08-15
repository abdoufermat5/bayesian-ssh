/**
 * Modal open/close state for the desktop app.
 *
 * Holds every modal's visibility flag plus the delete-confirmation target.
 * The `closeAllModals()` helper closes every modal at once (used by the
 * global Escape handler and by app shutdown paths).
 */

export interface DeleteConfirmTarget {
  title?: string;
  confirmLabel?: string;
  warning?: string;
  label: string;
  subtitle: string;
  onConfirm: () => Promise<void>;
}

let showModal = $state(false);
let showEnvModal = $state(false);
let showAgentModal = $state(false);
let showSessionManager = $state(false);
let showDeleteConfirm = $state(false);
let deleteTarget = $state<DeleteConfirmTarget | null>(null);

// Kerberos modal loading/error state (owned here because it is modal UI state).
let kerberosLoading = $state(false);
let kerberosError = $state<string | null>(null);

export function promptDelete(
  label: string,
  subtitle: string,
  onConfirm: () => Promise<void>,
  options?: { title?: string; confirmLabel?: string; warning?: string },
) {
  deleteTarget = { label, subtitle, onConfirm, ...options };
  showDeleteConfirm = true;
}

export async function confirmDelete() {
  if (!deleteTarget) return;
  showDeleteConfirm = false;
  try {
    await deleteTarget.onConfirm();
  } finally {
    deleteTarget = null;
  }
}

/** Close every owned modal. */
export function closeAllModals() {
  showModal = false;
  showEnvModal = false;
  showAgentModal = false;
  showSessionManager = false;
  showDeleteConfirm = false;
}

export function getModalsState() {
  return {
    get showModal() {
      return showModal;
    },
    set showModal(value: boolean) {
      showModal = value;
    },
    get showEnvModal() {
      return showEnvModal;
    },
    set showEnvModal(value: boolean) {
      showEnvModal = value;
    },
    get showAgentModal() {
      return showAgentModal;
    },
    set showAgentModal(value: boolean) {
      showAgentModal = value;
    },
    get showSessionManager() {
      return showSessionManager;
    },
    set showSessionManager(value: boolean) {
      showSessionManager = value;
    },
    get showDeleteConfirm() {
      return showDeleteConfirm;
    },
    set showDeleteConfirm(value: boolean) {
      showDeleteConfirm = value;
    },
    get deleteTarget() {
      return deleteTarget;
    },
    set deleteTarget(value: DeleteConfirmTarget | null) {
      deleteTarget = value;
    },
    get kerberosLoading() {
      return kerberosLoading;
    },
    set kerberosLoading(value: boolean) {
      kerberosLoading = value;
    },
    get kerberosError() {
      return kerberosError;
    },
    set kerberosError(value: string | null) {
      kerberosError = value;
    },
    promptDelete,
    confirmDelete,
    closeAllModals,
  };
}
