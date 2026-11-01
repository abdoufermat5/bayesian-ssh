import type {
  AiAnalysis,
  AiApprovalOutcome,
  AiApprovalRequest,
  AiDecision,
  AiRedaction,
  AiReportSummary,
  AiRiskTag,
  AiStep,
  AiStoredEvent,
  AiToolCall,
  AiToolName,
} from "$lib/types";

/** Plain-language names for the analyzer's risk tags. */
export const TAG_LABEL: Record<AiRiskTag, string> = {
  read: "Read",
  write: "Write",
  "service-change": "Service change",
  destructive: "Destructive",
  privilege: "Privilege",
  network: "Network",
  "exfil-risk": "Exfiltration risk",
  unknown: "Unclear",
};

/** Badge class for a tag. Colour only marks what needs a closer look. */
export function tagBadge(tag: AiRiskTag): string {
  if (tag === "destructive" || tag === "exfil-risk") return "badge-error";
  if (tag === "privilege" || tag === "unknown") return "badge-warning";
  return "badge-neutral";
}

export const TOOL_LABEL: Record<AiToolName, string> = {
  list_hosts: "List hosts",
  host_info: "Host info",
  observe: "Observe",
  read_file: "Read file",
  search_logs: "Search logs",
  run_readonly: "Run (read-only)",
  run_many: "Run on several hosts",
  propose_change: "Propose change",
  apply_change: "Apply change",
  edit_file: "Edit file",
  service: "Service",
  run: "Run command",
  undo: "Undo",
  status: "Status",
  request_access: "Request access",
};

export function toolLabel(tool: string): string {
  return TOOL_LABEL[tool as AiToolName] ?? tool;
}

/** The 8-character form of an id, as the CLI prints it: `change-696de2f2` becomes `696de2f2`. */
export function shortId(id: string): string {
  return id.replace(/^change-/, "").slice(0, 8);
}

export function formatCountdown(msLeft: number): string {
  if (msLeft <= 0) return "Expired";
  const total = Math.ceil(msLeft / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

export function formatClock(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  return new Date(t).toLocaleTimeString(undefined, { hour12: false });
}

// ---------------------------------------------------------------- steps

/** The one line a step is shown as (the command, the service action...). */
export function stepSummary(step: AiStep): string {
  const a = step.action;
  if ("run" in a) return a.run.command;
  if ("edit_file" in a) return `Edit ${a.edit_file.path}`;
  if ("service" in a) return `${a.service.action} service ${a.service.unit}`;
  return `Undo ${a.undo.step}`;
}

/** Hosts a set of steps touches, in first-seen order. */
export function stepHosts(steps: AiStep[]): string[] {
  const out: string[] = [];
  for (const s of steps) if (s.host && !out.includes(s.host)) out.push(s.host);
  return out;
}

// ---------------------------------------------------------------- diffs

export type DiffLineKind = "add" | "del" | "ctx" | "hunk" | "meta";
export interface DiffLine {
  kind: DiffLineKind;
  text: string;
}

/** Classify the lines of a unified diff; anything else is context. */
export function parseDiff(patch: string): DiffLine[] {
  const lines = patch.replace(/\r\n/g, "\n").split("\n");
  if (lines.at(-1) === "") lines.pop();
  return lines.map((text) => {
    if (text.startsWith("@@")) return { kind: "hunk", text };
    if (text.startsWith("+++") || text.startsWith("---") || text.startsWith("diff ") || text.startsWith("index "))
      return { kind: "meta", text };
    if (text.startsWith("+")) return { kind: "add", text };
    if (text.startsWith("-")) return { kind: "del", text };
    return { kind: "ctx", text };
  });
}

export function diffStats(lines: DiffLine[]): { added: number; removed: number } {
  return {
    added: lines.filter((l) => l.kind === "add").length,
    removed: lines.filter((l) => l.kind === "del").length,
  };
}

// ---------------------------------------------------------------- analysis

/** Union of the tags of several analyses, in the analyzer's order of severity. */
export function unionTags(analyses: (AiAnalysis | null | undefined)[]): AiRiskTag[] {
  const order = Object.keys(TAG_LABEL) as AiRiskTag[];
  const set = new Set<AiRiskTag>();
  for (const a of analyses) for (const t of a?.tags ?? []) set.add(t);
  return order.filter((t) => set.has(t));
}

/**
 * Analyses of a plan's steps, then verify, then rollback: the request lists
 * them in that order (it may list only the steps).
 */
export function planStepAnalyses(
  analyses: (AiAnalysis | null)[],
  steps: AiStep[],
  verify: AiStep[],
  rollback: AiStep[],
) {
  return {
    steps: steps.map((_, i) => analyses[i] ?? null),
    verify: verify.map((_, i) => analyses[steps.length + i] ?? null),
    rollback: rollback.map((_, i) => analyses[steps.length + verify.length + i] ?? null),
  };
}

// ---------------------------------------------------------------- grants

const GRANT_BLOCKED_TAGS: AiRiskTag[] = ["destructive", "privilege", "exfil-risk", "unknown"];

/**
 * Why "approve and allow for N minutes" is not offered, or null when it is.
 * Mirrors `build_grant` in `commands/ai.rs`, which enforces it.
 */
export function grantBlocker(request: AiApprovalRequest): string | null {
  const s = request.subject;
  if (s.kind !== "call") return "A grant covers a single call, not a plan or an access request.";
  const host = s.call.host;
  if (!host || /[*?[\]]/.test(host)) return "A grant needs one named host.";
  if ((s.call.tool === "run" || s.call.tool === "run_many") && !s.analysis) {
    return "This command was not analysed.";
  }
  if (s.analysis && (s.analysis.unknown_reason || s.analysis.tags.some((t) => GRANT_BLOCKED_TAGS.includes(t)))) {
    return "Destructive, privileged or unclear commands can't be granted.";
  }
  return null;
}

// ---------------------------------------------------------------- timeline

export interface CallResult {
  exit_code: number | null;
  redactions: AiRedaction[];
}

export type TimelineEntry =
  | {
      kind: "call";
      key: string;
      at: string;
      call: AiToolCall;
      decision: AiDecision;
      result: CallResult | null;
    }
  | { kind: "approval"; key: string; at: string; requestId: string; outcome: AiApprovalOutcome }
  | { kind: "change"; key: string; at: string; changeId: string; applied: boolean; undoPoint: string | null }
  | { kind: "note"; key: string; at: string; tone: "info" | "warning"; text: string };

/** Oldest first. Results are folded into the call they answer. */
export function buildTimeline(events: AiStoredEvent[]): TimelineEntry[] {
  const entries: TimelineEntry[] = [];
  const calls = new Map<string, Extract<TimelineEntry, { kind: "call" }>>();
  for (const { seq, at, event } of events) {
    const key = `e${seq}`;
    switch (event.event) {
      case "call": {
        const entry = { kind: "call" as const, key, at, call: event.call, decision: event.decision, result: null };
        calls.set(event.call.call_id, entry);
        entries.push(entry);
        break;
      }
      case "result": {
        const result = { exit_code: event.exit_code, redactions: event.redactions };
        const call = calls.get(event.call_id);
        if (call) call.result = result;
        else entries.push({ kind: "note", key, at, tone: "info", text: `Result for call ${event.call_id}` });
        break;
      }
      case "approval":
        entries.push({ kind: "approval", key, at, requestId: event.request_id, outcome: event.outcome });
        break;
      case "change":
        entries.push({
          kind: "change",
          key,
          at,
          changeId: event.change_id,
          applied: event.applied,
          undoPoint: event.undo_point,
        });
        break;
      case "paused":
        entries.push({ kind: "note", key, at, tone: "warning", text: `Run paused: ${event.reason}` });
        break;
      case "resumed":
        entries.push({ kind: "note", key, at, tone: "info", text: `Run resumed by ${event.by}` });
        break;
      case "run_ended":
        entries.push({ kind: "note", key, at, tone: "info", text: `Run ended: ${event.reason}` });
        break;
      case "run_started":
        entries.push({
          kind: "note",
          key,
          at,
          tone: "info",
          text: `Run started for ${event.on_behalf_of} with policy ${event.policy}`,
        });
        break;
    }
  }
  return entries;
}

export function redactionSummary(redactions: AiRedaction[]): string {
  const total = redactions.reduce((n, r) => n + r.count, 0);
  if (total === 0) return "";
  const kinds = redactions.map((r) => `${r.kind} ×${r.count}`).join(", ");
  return `${total} ${total === 1 ? "redaction" : "redactions"} (${kinds})`;
}

/** "2 h 5 min", "45 s": how long a run lasted. Empty when either end is missing or invalid. */
export function formatDuration(startIso: string | null, endIso: string | null): string {
  if (!startIso || !endIso) return "";
  const ms = Date.parse(endIso) - Date.parse(startIso);
  if (Number.isNaN(ms) || ms < 0) return "";
  const total = Math.round(ms / 1000);
  if (total < 60) return `${total} s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes} min ${total % 60} s`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ${minutes % 60} min`;
  return `${Math.floor(hours / 24)} d ${hours % 24} h`;
}

export function decisionLabel(decision: Pick<AiDecision, "decision">): { label: string; badge: string } {
  switch (decision.decision) {
    case "allow":
      return { label: "Allowed", badge: "badge-success" };
    case "ask":
      return { label: "Needs approval", badge: "badge-warning" };
    case "deny":
      return { label: "Denied", badge: "badge-error" };
  }
}

export function outcomeText(outcome: AiApprovalOutcome): string {
  switch (outcome.outcome) {
    case "approved":
      return `Approved by ${outcome.by} (${outcome.channel})${outcome.grant ? ", with a grant" : ""}`;
    case "denied":
      return `Denied by ${outcome.by}${outcome.reason ? `: ${outcome.reason}` : ""}`;
    case "expired":
      return "Expired without an answer";
  }
}

// ---------------------------------------------------------------- agent-sent arguments

/** A field of the agent's raw arguments, or undefined. The shape is untrusted, so it is checked. */
export function argField(args: unknown, key: string): unknown {
  return typeof args === "object" && args !== null && key in args ? Reflect.get(args, key) : undefined;
}

export function argString(args: unknown, key: string): string | null {
  const v = argField(args, key);
  return typeof v === "string" && v !== "" ? v : null;
}

export function argStrings(args: unknown, key: string): string[] {
  const v = argField(args, key);
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
}

// ---------------------------------------------------------------- reports list

export type ReportFilter = "all" | "changes" | "denials" | "approvals";

export const REPORT_FILTERS: { id: ReportFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "changes", label: "With changes" },
  { id: "denials", label: "With denials" },
  { id: "approvals", label: "With approvals" },
];

/** Whether a run matches the chip. */
export function reportMatchesFilter(r: AiReportSummary, filter: ReportFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "changes":
      return r.changes > 0;
    case "denials":
      return r.denials > 0;
    case "approvals":
      return (r.approvals ?? 0) > 0;
  }
}

/** Runs matching every word of `query` (client, run id, user, host, policy) and the chip. */
export function filterReports(reports: AiReportSummary[], query: string, filter: ReportFilter): AiReportSummary[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return reports.filter((r) => {
    if (!reportMatchesFilter(r, filter)) return false;
    if (words.length === 0) return true;
    const hay = [r.run_id, r.client, r.on_behalf_of, r.policy, ...(r.hosts ?? [])].join("\n").toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}
