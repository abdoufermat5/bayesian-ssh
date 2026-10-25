import type { TeamAudit, TeamInvite, TeamsInitError } from "$lib/types";

/** Base64 (no padding) of a SHA-256 digest: what follows `SHA256:`. */
const FINGERPRINT_BODY_LENGTH = 43;

/**
 * Normalize a pasted fingerprint: drop every whitespace character (chunked
 * read-backs and line wraps), accept a missing or lower-case `SHA256:`
 * prefix. Base64 is case-sensitive, so the body is left as is.
 */
export function normalizeFingerprint(input: string): string {
  const compact = input.replace(/\s+/g, "");
  if (!compact) return "";
  const body = compact.replace(/^sha256:/i, "");
  return `SHA256:${body}`;
}

/** Full-length comparison that doesn't stop at the first differing character. */
export function fingerprintsMatch(a: string, b: string): boolean {
  const x = normalizeFingerprint(a);
  const y = normalizeFingerprint(b);
  let diff = x.length ^ y.length;
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    diff |= (x.charCodeAt(i) || 0) ^ (y.charCodeAt(i) || 0);
  }
  return diff === 0 && x.length > 0;
}

/** True once the pasted value is long enough that a mismatch isn't just "still typing". */
export function fingerprintLooksComplete(input: string): boolean {
  return normalizeFingerprint(input).length >= "SHA256:".length + FINGERPRINT_BODY_LENGTH;
}

/** `SHA256:abcdefgh…` → { prefix: "SHA256:", chunks: ["abcd", "efgh", …] }. */
export function chunkFingerprint(fingerprint: string): { prefix: string; chunks: string[] } {
  const match = /^([A-Za-z0-9]+:)?(.*)$/.exec(fingerprint.trim());
  const prefix = match?.[1] ?? "";
  const body = match?.[2] ?? "";
  const chunks: string[] = [];
  for (let i = 0; i < body.length; i += 4) chunks.push(body.slice(i, i + 4));
  return { prefix, chunks };
}

/** "2030-01-02 09:30 UTC": audit artifacts and tooltips use absolute UTC. */
export function formatUtc(iso: string, seconds = false): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "—";
  const text = date.toISOString().replace("T", " ");
  return `${text.slice(0, seconds ? 19 : 16)} UTC`;
}

/** The message an admin sends to an invitee: desktop and CLI paths, plus every field. */
export function inviteMessage(team: string, invite: TeamInvite): string {
  return [
    `Join the "${team}" team on Bayesian SSH.`,
    "",
    'Desktop app: open Teams, choose "Join with invite" and paste this whole message.',
    "Command line:",
    `  bssh teams init --server ${invite.server_url} --email ${invite.email} --invite ${invite.token}`,
    `  bssh teams join ${invite.token} --fingerprint ${invite.admin_fingerprint}`,
    "",
    `Server: ${invite.server_url}`,
    `Email: ${invite.email}`,
    `Token: ${invite.token}`,
    `Admin fingerprint: ${invite.admin_fingerprint}`,
    `The token works once, until ${formatUtc(invite.expires_at)}.`,
  ].join("\n");
}

export interface ParsedInvite {
  server?: string;
  email?: string;
  token?: string;
  fingerprint?: string;
}

/**
 * Pull server, email, token and admin fingerprint out of a pasted invite:
 * the message above, the older two-line CLI invite, or anything in between.
 */
export function parseInvite(text: string): ParsedInvite {
  const field = (re: RegExp) => re.exec(text)?.[1]?.trim();
  const fingerprint =
    field(/admin fingerprint:\s*(\S+)/i) ?? field(/--fingerprint\s+(\S+)/) ?? field(/\b(SHA256:[A-Za-z0-9+/=]+)/);
  return {
    server: field(/^\s*server:\s*(\S+)/im) ?? field(/--server\s+(\S+)/),
    email: field(/^\s*email:\s*(\S+@\S+)/im) ?? field(/--email\s+(\S+)/),
    token: field(/^\s*token:\s*(\S+)/im) ?? field(/bssh teams join\s+(?!--)(\S+)/),
    fingerprint: fingerprint ? normalizeFingerprint(fingerprint) : undefined,
  };
}

/** Team id named in a join error whose admin fingerprint didn't match. */
export function teamIdFromJoinError(error: string): string | null {
  return /no admin device of team ([0-9a-f-]{36})/i.exec(error)?.[1] ?? null;
}

/**
 * The structured rejection `teams_init` throws, or null for anything else
 * (a string from another command, a network failure outside Tauri).
 */
export function teamsInitError(err: unknown): TeamsInitError | null {
  if (err && typeof err === "object" && "kind" in err) return err as TeamsInitError;
  return null;
}

/**
 * "The server sent a code to ana@example.com (valid 15 minutes)." A rejected
 * code mails nothing new: the user checks it or asks for a new one.
 */
export function emailCodeNotice(init: TeamsInitError): string {
  if (init.kind === "email_code_rejected") {
    return "That code is wrong, expired or already used. Check it, or send a new code.";
  }
  if (init.kind !== "email_code_sent") return "";
  const minutes = Math.max(1, Math.ceil(init.expires_in_secs / 60));
  const unit = minutes === 1 ? "minute" : "minutes";
  return `The server sent a code to ${init.email} (valid ${minutes} ${unit}).`;
}

export type TeamTab = "hosts" | "members" | "audit";

export type AuditVerdict = "verified" | "failed";

/** The backend verifies the whole log whatever the list shows, so a clean log is verified in full. */
export function auditVerdict(log: TeamAudit): AuditVerdict {
  return log.clean ? "verified" : "failed";
}

export const AUDIT_EVENT_LABEL: Record<TeamAudit["lines"][number]["kind"], string> = {
  session_start: "Session start",
  session_end: "Session end",
  vault_read: "Vault read",
  vault_write: "Vault write",
};

export const AUDIT_CHECK_LABEL: Record<TeamAudit["lines"][number]["check"], string> = {
  valid: "Valid",
  bad_signature: "Bad signature",
  unknown_device: "Unknown device",
  wrong_team: "Other team",
};

function csvCell(value: string | number | null | undefined): string {
  const text = value === null || value === undefined ? "" : String(value);
  return /[",\n\r]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** CSV of the audit lines, in the order shown, with raw RFC 3339 times (UTC). */
export function auditCsv(log: TeamAudit): string {
  const header = ["occurred_at", "received_at", "member", "device", "event", "host", "client_ip", "check"];
  const rows = log.lines.map((l) =>
    [l.occurred_at, l.received_at, l.who, l.device_name, l.kind, l.host, l.client_ip, l.check].map(csvCell).join(","),
  );
  return [header.join(","), ...rows].join("\n");
}

export function downloadText(filename: string, text: string, type = "text/csv") {
  const blob = new Blob([text], { type });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
