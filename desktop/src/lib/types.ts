export interface Connection {
  id: string;
  name: string;
  host: string;
  user: string;
  port: number;
  bastion?: string;
  bastion_user?: string;
  use_kerberos: boolean;
  key_path?: string;
  created_at: string;
  last_used?: string;
  tags: string[];
  source_id?: string;
  external_id?: string;
  stale_since?: string;
}

// "team" sources are bssh Teams vaults: created and synced by `bssh teams`, never added here.
export type SourceKind = "ssh-config" | "ansible" | "tailscale" | "aws" | "gcp" | "team";

export interface Source {
  id: string;
  name: string;
  kind: SourceKind;
  config: Record<string, unknown>;
  created_at: string;
  last_sync_at?: string;
  last_sync_error?: string;
  hosts?: number;
  stale?: number;
}

export interface SyncReport {
  added: number;
  updated: number;
  stale: number;
  unstaled: number;
  pruned: number;
  skipped_invalid: number;
  renamed: [string, string][];
  duplicates: string[];
  dry_run: boolean;
}

export interface EnvInfo {
  name: string;
  is_active: boolean;
}

export interface SessionHistoryEntry {
  session_id: string;
  connection_name: string;
  started_at: string;
  ended_at?: string;
  status: string | { Error: string };
  exit_code?: number;
  /** chrono::Duration serializes as [seconds, nanoseconds]. */
  duration?: [number, number] | number;
  recording_path?: string;
}

export interface SessionTextHit {
  session_id: string;
  connection_name: string;
  started_at: string;
  snippet: string;
}

export interface ConnectionStats {
  total_connections: number;
  most_used?: Connection;
  recently_used: Connection[];
  by_tag: Record<string, number>;
}

export interface DesktopSettings {
  theme: string;
  auto_start_agent: boolean;
  custom_agent_socket: string;
  kerberos_warn_minutes: number;
  monitor_kerberos: boolean;
  default_user: string;
  default_port: number;
  fuzzy_search: boolean;
  default_key_path: string;
  timezone: string;
  onboarding_complete?: boolean;
  terminal_font_family?: string;
  terminal_font_size?: number;
  terminal_line_height?: number;
  terminal_cursor_style?: "block" | "bar" | "underline";
  terminal_cursor_blink?: boolean;
  terminal_copy_on_select?: boolean;
  terminal_smart_copy?: boolean;
  terminal_scrollback?: number;
  sftp_show_hidden_files?: boolean;
  sftp_default_remote_path?: string;
  enable_sftp?: boolean;
  record_sessions?: boolean;
  record_tags?: string[];
  density?: "compact" | "comfortable" | "spacious";
}

export interface WorkspaceInfo {
  active_env: string;
  config_root: string;
  env_dir: string;
  config_path: string;
  database_path: string;
  ssh_config_path?: string | null;
  default_user: string;
  default_port: number;
  search_mode: string;
  log_level: string;
  auto_save_history: boolean;
  max_history_size: number;
}

export interface OnboardingPayload {
  profile_name: string;
  create_profile: boolean;
  default_user: string;
  default_port: number;
  ssh_config_path?: string | null;
  theme: string;
  auto_start_agent: boolean;
  import_ssh_config: boolean;
  fuzzy_search: boolean;
}

export type AppTab =
  | "connections"
  | "terminals"
  | "sftp"
  | "history"
  | "keys"
  | "audit"
  | "teams"
  | "ai"
  | "settings";
export type NotificationType = "success" | "error" | "info";

export interface SshKeyInfo {
  name: string;
  key_type: string;
  fingerprint: string;
  comment: string;
  public_key_path: string;
  private_key_path?: string;
  private_key_permission: string;
  is_secure: boolean;
}

export interface AuditFinding {
  severity: "critical" | "warning" | "info";
  title: string;
  description: string;
  remediation: string;
}

export interface AuditReport {
  score: number;
  grade: string;
  rating: string;
  findings: AuditFinding[];
  total_critical: number;
  total_warning: number;
  total_info: number;
}

/** Output of `check_update`; null when this build is current. */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
}

/**
 * Who owns updates for this build. `"snap"`: the Snap Store installs them.
 * `"manual"`: raw binary / unified package / source build, updated the way it
 * was installed. `null`: the in-app updater applies.
 */
export type UpdateChannel = "snap" | "manual";

/** Outcome of one local file/directory upload to one host. */
export interface HostUploadResult {
  connectionId: string;
  name: string;
  host: string;
  ok: boolean;
  error?: string | null;
  files: number;
  bytes: number;
  viaScp: boolean;
  /** Final remote path the upload resolved to. */
  remotePath: string;
  elapsedMs: number;
}

/** Streaming event emitted by `upload-event` (tagged by `kind`). */
export type UploadEvent =
  | { kind: "started"; connectionId: string; name: string; host: string }
  | { kind: "progress"; connectionId: string; bytesDone: number; bytesTotal?: number | null }
  | { kind: "finished"; result: HostUploadResult };

/** Payload of the `upload-event` Tauri event. */
export interface UploadEventPayload {
  jobId: string;
  localPath: string;
  event: UploadEvent;
}

/** Payload of the `upload-finished` Tauri event. */
export interface UploadFinishedPayload {
  jobId: string;
  cancelled: boolean;
  results: Array<{ localPath: string; hosts: HostUploadResult[] }>;
  error?: string | null;
}

export interface ScoreBreakdown {
  prior: number;
  likelihood: number;
  recency: number;
  success: number;
  repo?: number | null;
  hour?: number | null;
  after_prev?: number | null;
  context_factor: number;
  repo_hits: number;
  hour_hits: number;
  after_prev_hits: number;
  total_w: number;
}

export interface Prediction {
  connection: Connection;
  score: number;
  breakdown: ScoreBreakdown;
}

// ── bssh Teams (crates/gui/src/commands/teams.rs) ──────────────────────────

export interface TeamsIdentity {
  email: string;
  server_url: string;
  device_id: string;
}

export interface TeamSummary {
  team_id: string;
  /** The team's inventory source; its connections carry this source_id. */
  source_id: string;
  name: string;
  hosts: number;
  last_sync_at?: string;
}

export interface TeamsStatus {
  identity?: TeamsIdentity;
  unlocked: boolean;
  fingerprint?: string;
  teams: TeamSummary[];
}

export interface TeamSyncReport {
  team_id: string;
  name?: string;
  waiting_for_key: boolean;
  pulled: number;
  pushed: number;
  conflicts_resolved: number;
  audit_uploaded: number;
  dropped_edits: number;
  rejected: string[];
  unreadable: number;
  rotated_to?: number;
  rotation_required: boolean;
  errors: string[];
  hosts_added: number;
  hosts_updated: number;
  hosts_stale: number;
}

export interface TeamSyncResult {
  team: string;
  result?: TeamSyncReport;
  error?: string;
}

export interface TeamInvite {
  email: string;
  expires_at: string;
  token: string;
  admin_fingerprint: string;
  server_url: string;
}

export interface TeamJoin {
  team_id: string;
  name: string;
  my_fingerprint: string;
}

export interface TeamDevice {
  name: string;
  fingerprint: string;
}

export interface TeamPendingDevice {
  email: string;
  device: TeamDevice;
}

/** A pending invite, as its team's admin sees it (no token). */
export interface TeamPendingInvite {
  email: string;
  role: "admin" | "member";
  expires_at: string;
}

export interface TeamMember {
  email: string;
  role: "admin" | "member";
  devices: TeamDevice[];
}

export interface TeamAuditLine {
  occurred_at: string;
  received_at: string;
  kind: "session_start" | "session_end" | "vault_read" | "vault_write";
  who: string;
  device_name: string;
  host?: string;
  client_ip?: string;
  check: "valid" | "bad_signature" | "unknown_device" | "wrong_team";
}

export interface TeamAudit {
  lines: TeamAuditLine[];
  chains: number;
  from_genesis: boolean;
  problems: string[];
  invalid_events: number;
  complete: boolean;
  clean: boolean;
}

// ── bssh Teams, UX phase 1 contract (backend: crates/gui/src/commands/teams.rs) ──

export type TeamRole = "admin" | "member";

/**
 * `teams_attention()` (needs Teams unlocked; one network round per team).
 * What needs a human for each local team.
 */
export interface TeamAttention {
  team_id: string;
  role: TeamRole;
  /** Devices waiting for the key; always 0 for members (they must not see this). */
  pending_devices: number;
  /** This device has no team key yet: an admin must grant it. */
  waiting_for_key: boolean;
  /** This account was removed from the team; it can no longer sync. */
  removed: boolean;
}

/** `teams_billing(team)`: the team's plan and seats (server `BillingInfo`). */
export interface TeamBillingInfo {
  /** False on a server without billing: `seat_limit` is fixed. */
  billing_enabled: boolean;
  plan: "free" | "team";
  free_seats: number;
  paid_seats: number;
  /** What invites are checked against: members plus pending invites. */
  seat_limit: number;
  seats_used: number;
  /** Stripe's subscription status (active, past_due, canceled…). */
  status: string | null;
  interval: "month" | "year" | null;
  current_period_end: string | null;
  cancel_at_period_end: boolean;
  /** More members than the free seats and no active subscription: changes are refused. */
  read_only: boolean;
  /** Set when the server has organizations: seats are counted for this organization. */
  org_id?: string | null;
}

/** `teams_orgs()` / `teams_org(org)`: an organization (backend `OrgDto`). It has no name: label it by its owners. */
export interface OrgSummary {
  org_id: string;
  short_id: string;
  /** The owners' emails, e.g. "ana@example.com, bo@example.com". */
  label: string;
  my_role: "owner" | "member";
  owners: { account_id: string; email: string }[];
  /** `name` is the local name when this machine knows the team. */
  teams: { team_id: string; name: string | null; short_id: string }[];
  created_at: string;
}

export interface TeamPreviewDevice {
  email: string;
  name: string;
  fingerprint: string;
  /** This admin machine approved this device (granted it a key). */
  approved: boolean;
}

/**
 * `teams_remove_preview({ team, email })`: what removing a member would do,
 * computed before anything changes. Rotation only wraps the new key to
 * approved devices; any `blocking` device makes the rotation fail.
 */
export interface TeamRemovePreview {
  email: string;
  /** Devices of the member being removed. */
  removed_devices: TeamPreviewDevice[];
  /** Key version after rotation (current + 1). */
  next_key_version: number;
  /** Remaining devices that will receive the new key. */
  recipients: TeamPreviewDevice[];
  /** Remaining devices this machine never approved: rotation would stop. */
  blocking: TeamPreviewDevice[];
  /** Team hosts the removed member has already synced (they keep copies). */
  shared_hosts: number;
}

// ── bssh AI (backend: crates/gui/src/commands/ai.rs; shapes of crates/cli/src/services/ai/types.rs) ──

export type AiRiskTag =
  | "read"
  | "write"
  | "service-change"
  | "destructive"
  | "privilege"
  | "network"
  | "exfil-risk"
  | "unknown";

export type AiTier = "in-client" | "out-of-band" | "two-person";

export type AiToolName =
  | "list_hosts"
  | "host_info"
  | "observe"
  | "read_file"
  | "search_logs"
  | "run_readonly"
  | "run_many"
  | "propose_change"
  | "apply_change"
  | "edit_file"
  | "service"
  | "run"
  | "undo"
  | "status"
  | "request_access";

export interface AiSimpleCommand {
  program: string;
  args: string[];
  wrappers: string[];
  writes_to: string[];
  tags: AiRiskTag[];
  rule: string | null;
}

export interface AiAnalysis {
  commands: AiSimpleCommand[];
  tags: AiRiskTag[];
  unknown_reason: string | null;
}

export interface AiToolCall {
  run_id: string;
  call_id: string;
  tool: AiToolName;
  host: string | null;
  command: string | null;
  path: string | null;
  /** Raw arguments exactly as the agent sent them. Untrusted. */
  args: unknown;
}

export type AiDecision =
  | { decision: "allow"; reason: string }
  | { decision: "ask"; tier: AiTier; reason: string }
  | { decision: "deny"; reason: string };

export type AiStepAction =
  | { run: { command: string } }
  | { edit_file: { path: string; patch: string } }
  | { service: { unit: string; action: "start" | "stop" | "restart" | "reload" } }
  | { undo: { step: string } };

export interface AiStep {
  id: string;
  host: string | null;
  action: AiStepAction;
}

export interface AiChangePlan {
  change_id: string;
  run_id: string;
  title: string;
  /** Written by the agent: show it labelled "The agent says". */
  reason: string;
  steps: AiStep[];
  verify: AiStep[];
  rollback: AiStep[];
}

export interface AiScope {
  tags: string[];
  hosts: string[];
}

export type AiApprovalSubject =
  | { kind: "call"; call: AiToolCall; analysis: AiAnalysis | null }
  | { kind: "plan"; plan: AiChangePlan; plan_hash: string; analyses: (AiAnalysis | null)[] }
  | { kind: "access"; scope: AiScope; reason: string; duration: string };

export interface AiApprovalRequest {
  request_id: string;
  run_id: string;
  /** MCP client name and version, self-reported. */
  client: string;
  tier: AiTier;
  subject: AiApprovalSubject;
  created_at: string;
  expires_at: string;
}

export interface AiGrant {
  grant_id: string;
  run_id: string | null;
  tool: AiToolName;
  hosts: string;
  target: string | null;
  expires_at: string;
}

export type AiApprovalOutcome =
  | { outcome: "approved"; by: string; channel: string; grant: AiGrant | null }
  | { outcome: "denied"; by: string; reason: string | null }
  | { outcome: "expired" };

export interface AiRedaction {
  kind: string;
  count: number;
}

export type AiEvent =
  | { event: "run_started"; run_id: string; client: string; on_behalf_of: string; policy: string; policy_hash: string }
  | { event: "call"; call: AiToolCall; decision: AiDecision }
  | { event: "approval"; request_id: string; run_id: string; outcome: AiApprovalOutcome }
  | {
      event: "result";
      run_id: string;
      call_id: string;
      exit_code: number | null;
      output_sha256: string;
      redactions: AiRedaction[];
    }
  | {
      event: "change";
      run_id: string;
      change_id: string;
      plan_hash: string;
      applied: boolean;
      undo_point: string | null;
    }
  | { event: "circuit_breaker"; run_id: string; reason: string }
  | { event: "run_ended"; run_id: string; reason: string };

/** One event of a run as the desktop received it (`ai_run_events`, `ai://event`). */
export interface AiStoredEvent {
  /** 1-based per run; used to drop duplicates. */
  seq: number;
  /** RFC 3339, when the desktop received it. */
  at: string;
  event: AiEvent;
}

/** `ai_list_runs()` and the `ai://runs` event. */
export interface AiRun {
  run_id: string;
  client: string;
  policy: string;
  started_at: string;
  on_behalf_of: string | null;
  /** Calls seen since the desktop connected. */
  calls: number;
  denied: number;
  pending: AiApprovalRequest[];
  grants: AiGrant[];
  /** The run's socket is open: Stop and approvals work. */
  connected: boolean;
  /** Reason from the run's `run_ended` event. */
  ended: string | null;
}

export interface AiBudgets {
  calls_per_min: number;
  changes_per_run: number;
  run_ttl: string;
  max_denials_per_min: number;
}

/** `config.ai.policies[name]`. */
export interface AiPolicy {
  profile: "observe" | "operate" | "oncall";
  scope: AiScope;
  approval: { default: AiTier; in_client: string[]; two_person: string[] };
  read_paths: string[];
  deny_paths: string[];
  write_paths: string[];
  deny: string[];
  budgets: AiBudgets;
  redact: { vault_secrets: boolean; patterns: string };
}

/** `ai_settings()`: the read-only part of `config.ai`. */
export interface AiSettingsView {
  policies: Record<string, AiPolicy>;
  record_output: boolean;
  approval_timeout_secs: number;
}
