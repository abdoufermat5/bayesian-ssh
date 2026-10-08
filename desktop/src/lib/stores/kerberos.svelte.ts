import { invoke } from "@tauri-apps/api/core";
import type { Connection } from "$lib/types";

export interface KerberosStatus {
  tools_available: boolean;
  client_configured: boolean;
  has_ticket: boolean;
  valid: boolean;
  principal: string | null;
  suggested_principal: string | null;
  default_realm: string | null;
  config_path: string | null;
  cache_path: string | null;
  expires_at: number | null;
  renew_until: number | null;
  renewable: boolean;
  seconds_remaining: number | null;
}

const EMPTY_STATUS: KerberosStatus = {
  tools_available: false,
  client_configured: false,
  has_ticket: false,
  valid: false,
  principal: null,
  suggested_principal: null,
  default_realm: null,
  config_path: null,
  cache_path: null,
  expires_at: null,
  renew_until: null,
  renewable: false,
  seconds_remaining: null,
};

let status = $state<KerberosStatus>(EMPTY_STATUS);
let showModal = $state(false);
let pendingConnection = $state<Connection | null>(null);
let ticketLifetimeSeconds = $state<number | null>(null);
let liveRemainingSeconds = $state<number | null>(null);

// Internal bookkeeping: never read by templates, so plain variables.
let expiresAtMs: number | null = null;
let warnMinutes = 15;
let warnedForExpiry: number | null = null;
let onExpiryWarning: ((message: string) => void) | null = null;

/** klist is spawned at most once a minute, and only while the window is visible. */
const POLL_MS = 60_000;
/** The countdown ticks every second only in the final hour; otherwise every 30 s. */
const FAST_TICK_MS = 1_000;
const SLOW_TICK_MS = 30_000;
const FAST_TICK_WINDOW_S = 3600;

let monitoring = false;
/** Set only by a successful status call: no Kerberos client on this machine. */
let toolsMissing = false;
let pollTimer: number | undefined;
let tickTimer: number | undefined;
let visibilityListenerAttached = false;

function isDocumentHidden(): boolean {
  return typeof document !== "undefined" && document.visibilityState === "hidden";
}

function updateLiveRemainingSeconds(): number | null {
  if (!expiresAtMs) {
    liveRemainingSeconds = status.seconds_remaining;
  } else {
    liveRemainingSeconds = Math.max(0, Math.floor((expiresAtMs - Date.now()) / 1000));
  }
  return liveRemainingSeconds;
}

function syncExpiryFromStatus(next: KerberosStatus) {
  expiresAtMs = next.expires_at ? next.expires_at * 1000 : null;
  if (next.has_ticket && next.seconds_remaining && next.seconds_remaining > 0) {
    ticketLifetimeSeconds = Math.max(ticketLifetimeSeconds ?? 0, next.seconds_remaining);
  }
  if (!next.has_ticket || !next.valid) {
    ticketLifetimeSeconds = null;
  }
  updateLiveRemainingSeconds();
  scheduleTick();
}

/** Next klist poll; skipped while hidden or when Kerberos tools are absent. */
function schedulePoll() {
  clearTimeout(pollTimer);
  if (!monitoring || toolsMissing || isDocumentHidden()) return;
  pollTimer = window.setTimeout(async () => {
    await refreshKerberosStatus();
    schedulePoll();
  }, POLL_MS);
}

/**
 * Countdown tick. Runs only while a ticket with a known expiry exists. The
 * delay never overshoots the warning threshold or the expiry, so warnings
 * (and tray notifications while hidden) still fire on time.
 */
function scheduleTick() {
  clearTimeout(tickTimer);
  if (!monitoring || expiresAtMs === null) return;
  const remaining = liveRemainingSeconds;
  if (remaining !== null && remaining <= 0) return; // expired: already warned; polls pick up renewals

  const fast = !isDocumentHidden() && remaining !== null && remaining < FAST_TICK_WINDOW_S;
  let delay = fast ? FAST_TICK_MS : SLOW_TICK_MS;
  if (remaining !== null) {
    const untilWarn = remaining - warnMinutes * 60;
    if (untilWarn > 0) delay = Math.min(delay, untilWarn * 1000);
    delay = Math.min(delay, remaining * 1000);
  }

  tickTimer = window.setTimeout(() => {
    maybeWarnExpiry(updateLiveRemainingSeconds());
    scheduleTick();
  }, Math.max(delay, FAST_TICK_MS));
}

function handleVisibilityChange() {
  if (!monitoring) return;
  if (isDocumentHidden()) {
    clearTimeout(pollTimer);
    scheduleTick(); // drop to the slow cadence
    return;
  }
  // Visible again: refresh immediately, then resume the regular cadence.
  if (toolsMissing) return;
  void refreshKerberosStatus().then(schedulePoll);
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
  return liveRemainingSeconds;
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
    toolsMissing = !next.tools_available;
    syncExpiryFromStatus(next);
    maybeWarnExpiry(getLiveRemainingSeconds());
    return next;
  } catch {
    // Transient IPC failure: clear the ticket view but keep polling.
    status = EMPTY_STATUS;
    syncExpiryFromStatus(EMPTY_STATUS);
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
  proxiable = false,
  lifetime?: string,
  renewLifetime?: string,
): Promise<KerberosStatus> {
  const next = await invoke<KerberosStatus>("acquire_kerberos_ticket", {
    principal: principal?.trim() || null,
    password,
    forwardable,
    proxiable,
    lifetime: lifetime?.trim() || null,
    renew_lifetime: renewLifetime?.trim() || null,
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
  // Polling is slow (and paused while hidden); show fresh data on open. This
  // also restarts polling if Kerberos tools were installed since the last check.
  void refreshKerberosStatus().then(schedulePoll);
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
  // Preserve an existing warning handler when none is provided, so a caller
  // that only passes warnMinutes does not silently drop the notification
  // handler (appState.handleKerberosWarning).
  if (options?.onWarning !== undefined) {
    onExpiryWarning = options.onWarning;
  }

  stopKerberosMonitoring();
  monitoring = true;
  if (!visibilityListenerAttached && typeof document !== "undefined") {
    document.addEventListener("visibilitychange", handleVisibilityChange);
    visibilityListenerAttached = true;
  }
  // The first status decides whether polling is needed at all.
  void refreshKerberosStatus().then(schedulePoll);
}

export function stopKerberosMonitoring() {
  monitoring = false;
  clearTimeout(pollTimer);
  clearTimeout(tickTimer);
  if (visibilityListenerAttached) {
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    visibilityListenerAttached = false;
  }
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
