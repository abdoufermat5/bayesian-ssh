import type { Prediction } from "$lib/types";
import { formatRelative } from "$lib/utils/timezone";

/**
 * One-line explanation of why a connection is suggested, from the strongest
 * context factor that pushed it up. Shared by the Hosts strip and the command
 * palette so both say the same thing.
 */
export function predictionReason(p: Prediction, now = Date.now()): string {
  const b = p.breakdown;
  const candidates: Array<{ key: "repo" | "after_prev" | "hour"; lr: number }> = [];
  if (b.repo && b.repo > 1 && b.repo_hits > 0) candidates.push({ key: "repo", lr: b.repo });
  if (b.after_prev && b.after_prev > 1 && b.after_prev_hits > 0) candidates.push({ key: "after_prev", lr: b.after_prev });
  if (b.hour && b.hour > 1 && b.hour_hits > 0) candidates.push({ key: "hour", lr: b.hour });
  candidates.sort((a, c) => c.lr - a.lr);

  switch (candidates[0]?.key) {
    case "repo":
      return `${b.repo_hits} of ${b.total_w} sessions from this repo`;
    case "after_prev":
      return "Often opened right after your last session";
    case "hour":
      return `Often used around ${String(new Date(now).getHours()).padStart(2, "0")}:00`;
  }

  const last = p.connection.last_used ? formatRelative(p.connection.last_used, now) : null;
  if (b.total_w > 0) return last ? `Used ${b.total_w}× · last ${last}` : `Used ${b.total_w}×`;
  return last ? `Last used ${last}` : "Suggested";
}

/** Tooltip with the factors behind a suggestion's score. */
export function predictionDetail(p: Prediction): string {
  const b = p.breakdown;
  const parts = [`${b.total_w} recent session${b.total_w === 1 ? "" : "s"}`];
  if (b.repo) parts.push(`repo ×${b.repo.toFixed(1)}`);
  if (b.hour) parts.push(`hour ×${b.hour.toFixed(1)}`);
  if (b.after_prev) parts.push(`after last host ×${b.after_prev.toFixed(1)}`);
  return `${p.connection.user}@${p.connection.host} — ${parts.join(" · ")}`;
}
