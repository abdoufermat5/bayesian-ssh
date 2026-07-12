import { invoke } from "@tauri-apps/api/core";
import type { Connection } from "$lib/types";

export interface KerberosStatus {
  tools_available: boolean;
  has_ticket: boolean;
  valid: boolean;
  principal: string | null;
  cache_path: string | null;
  expires_at: number | null;
  renew_until: number | null;
  renewable: boolean;
  seconds_remaining: number | null;
}

const EMPTY_STATUS: KerberosStatus = {
  tools_available: false,
  has_ticket: false,
  valid: false,
  principal: null,
  cache_path: null,
  expires_at: null,
  renew_until: null,
  renewable: false,
  seconds_remaining: null,
};

let status = $state<KerberosStatus>(EMPTY_STATUS);
let showModal = $state(false);
let pendingConnection = $state<Connection | null>(null);
let expiresAtMs = $state<number | null>(null);
let ticketLifetimeSeconds = $state<number | null>(null);
let pollTimer: ReturnType<typeof setInterval> | null = null;
let tickTimer: ReturnType<typeof setInterval> | null = null;
let warnMinutes = $state(15);
let warnedForExpiry = $state<number | null>(null);
let onExpiryWarning: ((message: string) => void) | null = null;

function syncExpiryFromStatus(next: KerberosStatus) {
  expiresAtMs = next.expires_at ? next.expires_at * 1000 : null;
  if (next.has_ticket && next.seconds_remaining && next.seconds_remaining > 0) {
    ticketLifetimeSeconds = Math.max(ticketLifetimeSeconds ?? 0, next.seconds_remaining);
  }
  if (!next.has_ticket || !next.valid) {
    ticketLifetimeSeconds = null;
  }
}

export function formatKerberosRemaining(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "—";
  if (seconds <= 0) return "Expired";

  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;

  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${secs}s`;
  return `${secs}s`;
}

export function getLiveRemainingSeconds(): number | null {
  if (!expiresAtMs) return status.seconds_remaining;
  return Math.max(0, Math.floor((expiresAtMs - Date.now()) / 1000));
}

export function getKerberosHealth(
  remaining: number | null,
  thresholdMinutes: number,
): "missing" | "expired" | "warning" | "valid" | "unavailable" {
  if (!status.tools_available) return "unavailable";
  if (!status.has_ticket) return "missing";
  if (remaining === null || remaining <= 0 || !status.valid) return "expired";
  if (remaining <= thresholdMinutes * 60) return "warning";
  return "valid";
}

function maybeWarnExpiry(remaining: number | null) {
  if (!onExpiryWarning || remaining === null) return;
  if (remaining <= 0) {
    if (warnedForExpiry !== 0) {
      warnedForExpiry = 0;
      onExpiryWarning("Kerberos ticket expired. Renew to keep SSH sessions alive.");
      showModal = true;
    }
    return;
  }

  const threshold = warnMinutes * 60;
  if (remaining <= threshold && warnedForExpiry !== status.expires_at) {
    warnedForExpiry = status.expires_at;
    onExpiryWarning(
      `Kerberos ticket expires in ${formatKerberosRemaining(remaining)}. Renew now to avoid disconnects.`,
    );
    showModal = true;
  }
}

export async function refreshKerberosStatus(): Promise<KerberosStatus> {
  try {
    const next = await invoke<KerberosStatus>("get_kerberos_status");
    status = next;
    syncExpiryFromStatus(next);
    maybeWarnExpiry(getLiveRemainingSeconds());
    return next;
  } catch {
    status = EMPTY_STATUS;
    expiresAtMs = null;
    return EMPTY_STATUS;
  }
}

export async function renewKerberosTicket(password?: string): Promise<KerberosStatus> {
  const next = await invoke<KerberosStatus>("renew_kerberos_ticket", {
    password: password?.trim() ? password : null,
  });
  status = next;
  syncExpiryFromStatus(next);
  warnedForExpiry = null;
  return next;
}

export async function acquireKerberosTicket(
  password: string,
  principal?: string,
  forwardable = true,
): Promise<KerberosStatus> {
  const next = await invoke<KerberosStatus>("acquire_kerberos_ticket", {
    principal: principal?.trim() || null,
    password,
    forwardable,
  });
  status = next;
  syncExpiryFromStatus(next);
  warnedForExpiry = null;
  return next;
}

export async function ensureKerberosForConnection(conn: Connection): Promise<boolean> {
  if (!conn.use_kerberos) return true;

  const current = await refreshKerberosStatus();
  const remaining = getLiveRemainingSeconds();
  if (current.valid && (remaining === null || remaining > 0)) {
    return true;
  }

  pendingConnection = conn;
  showModal = true;
  return false;
}

export function openKerberosModal() {
  showModal = true;
}

export function closeKerberosModal() {
  showModal = false;
  pendingConnection = null;
}

export function consumePendingConnection(): Connection | null {
  const conn = pendingConnection;
  pendingConnection = null;
  return conn;
}

export function startKerberosMonitoring(options?: {
  warnMinutes?: number;
  onWarning?: (message: string) => void;
}) {
  if (options?.warnMinutes !== undefined) {
    warnMinutes = options.warnMinutes;
  }
  onExpiryWarning = options?.onWarning ?? null;

  stopKerberosMonitoring();
  void refreshKerberosStatus();

  pollTimer = setInterval(() => {
    void refreshKerberosStatus();
  }, 30_000);

  tickTimer = setInterval(() => {
    const remaining = getLiveRemainingSeconds();
    maybeWarnExpiry(remaining);
  }, 1_000);
}

export function stopKerberosMonitoring() {
  if (pollTimer) clearInterval(pollTimer);
  if (tickTimer) clearInterval(tickTimer);
  pollTimer = null;
  tickTimer = null;
}

export function getKerberosState() {
  return {
    get status() {
      return status;
    },
    get showModal() {
      return showModal;
    },
    set showModal(value: boolean) {
      showModal = value;
    },
    get pendingConnection() {
      return pendingConnection;
    },
    get liveRemainingSeconds() {
      return getLiveRemainingSeconds();
    },
    get ticketLifetimeSeconds() {
      return ticketLifetimeSeconds;
    },
  };
}
