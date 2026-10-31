//! Read tools: `observe`, `read_file` and `search_logs`.

use super::convert::{exit_text, reds, to_value};
use super::exec::exec_out;
use super::inputs::{ObserveArgs, ReadFileArgs, SearchLogsArgs};
use super::outputs::{FileOut, NOTICE, ObserveOut};
use crate::models::Connection;
use crate::services::ai::mcp::context::{RunContext, ToolError, ToolResult, merge_redactions};
use crate::services::ai::mcp::util::{normalize_path, sh_quote, valid_since, valid_unit};
use crate::services::ai::observe;
use crate::services::ai::types::{ExecLimits, ExecMode, ToolName};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Default and largest `read_file` window.
pub const READ_DEFAULT: u64 = 64 * 1024;
pub const READ_MAX: u64 = 256 * 1024;
/// Bound on the `readlink -f` that resolves a `read_file` target on the host.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(15);
const RESOLVE_MAX_BYTES: usize = 8 * 1024;
const LOG_LINES_DEFAULT: u32 = 200;
const LOG_LINES_MAX: u32 = 1000;

pub(super) async fn observe_tool(ctx: &Arc<RunContext>, a: ObserveArgs) -> ToolResult {
    let conn = ctx.resolve_host(&a.host).await?;
    let facts = ctx.facts(&conn).await;
    let cmd = observe::command(a.what);
    let call = ctx.new_call(
        ToolName::Observe,
        Some(&conn.name),
        Some(cmd.into()),
        None,
        json!({ "host": a.host, "what": a.what.as_str() }),
    );
    ctx.authorize(&call, None, &facts).await?;
    let ex = ctx
        .exec(
            &conn,
            &call,
            cmd,
            RunContext::read_mode(&facts),
            ExecLimits::default(),
        )
        .await?;
    if ex.result.timed_out {
        return Err(ToolError::FailedAfterRun(format!(
            "observe {} timed out",
            a.what.as_str()
        )));
    }
    let mut data = observe::parse(a.what, &ex.result.stdout).map_err(|e| {
        ToolError::FailedAfterRun(format!(
            "could not read {} output from {} ({}): {e}",
            a.what.as_str(),
            conn.name,
            exit_text(ex.result.exit_code)
        ))
    })?;
    // Parsed strings (process arguments, unit descriptions...) may still carry secrets.
    let more = ctx.redact_value(&mut data);
    let all = merge_redactions([ex.redactions.clone(), more]);
    to_value(ObserveOut {
        host: conn.name.clone(),
        what: a.what.as_str().into(),
        untrusted_data: data,
        redactions: reds(&all),
        notice: NOTICE.into(),
    })
}

/// Why a `read_file` target could not be resolved on the host.
#[derive(Debug, thiserror::Error)]
enum ResolveError {
    #[error("could not resolve the path: {0}")]
    Exec(String),
    #[error("could not resolve the path: timed out")]
    TimedOut,
    #[error("could not resolve the path: output too long")]
    OutputTooLong,
    #[error("could not resolve the path on the host (readlink: {0})")]
    Readlink(String),
    #[error("could not resolve the path: unexpected output")]
    UnexpectedOutput,
    #[error("resolved path '{path}': {reason}")]
    InvalidPath { path: String, reason: String },
}

/// Resolve `path` on the host with `readlink -f -- <path>`, as the connection's user.
///
/// The deny/read checks in [`read_file`] are lexical, so a symlink inside a readable
/// directory could point at a secret. The target is resolved first and the read then uses
/// the resolved path. Anything but one absolute path back is an error: a caller decides
/// that as a refusal, never as an allow.
async fn resolve_path(
    ctx: &Arc<RunContext>,
    conn: &Connection,
    path: &str,
) -> Result<String, ResolveError> {
    let cmd = format!("readlink -f -- {}", sh_quote(path));
    let limits = ExecLimits {
        timeout: RESOLVE_TIMEOUT,
        max_output_bytes: RESOLVE_MAX_BYTES,
    };
    let res = ctx
        .deps
        .executor
        .exec(conn, &cmd, ExecMode::Plain, limits)
        .await
        .map_err(|e| ResolveError::Exec(format!("{e:#}")))?;
    if res.timed_out {
        return Err(ResolveError::TimedOut);
    }
    if res.truncated {
        return Err(ResolveError::OutputTooLong);
    }
    if res.exit_code != Some(0) {
        return Err(ResolveError::Readlink(exit_text(res.exit_code)));
    }
    // The host's answer is untrusted: exactly one absolute path, no extra lines.
    let out = res.stdout.trim_end_matches(['\n', '\r']);
    if out.is_empty() || out.contains(['\n', '\r']) {
        return Err(ResolveError::UnexpectedOutput);
    }
    normalize_path(out).map_err(|e| ResolveError::InvalidPath {
        path: out.to_string(),
        reason: e.to_string(),
    })
}

pub(super) async fn read_file(ctx: &Arc<RunContext>, a: ReadFileArgs) -> ToolResult {
    let requested = ctx.clean_path(&a.path)?;
    let (offset, length) = match &a.range {
        Some(r) => (r.offset, r.length.clamp(1, READ_MAX)),
        None => (0, READ_DEFAULT),
    };
    let call = ctx.new_call(
        ToolName::ReadFile,
        Some(&a.host),
        None,
        Some(requested.clone()),
        json!({ "host": a.host, "path": requested, "offset": offset, "length": length }),
    );
    if let Some(why) = ctx.path_denied(&requested) {
        return Err(ctx.deny_call(&call, &format!("{requested}: {why}")));
    }
    let conn = ctx.resolve_host(&a.host).await?;
    let facts = ctx.facts(&conn).await;
    // Only the host knows what a symlink points at; the checks so far are lexical.
    let path = match resolve_path(ctx, &conn, &requested).await {
        Ok(p) => p,
        Err(why) => return Err(ctx.deny_call(&call, &format!("{requested}: {why}"))),
    };
    // The policy and the deny paths judge the file that is actually read, not the link.
    let call = ctx.new_call(
        ToolName::ReadFile,
        Some(&a.host),
        None,
        Some(path.clone()),
        json!({
            "host": a.host,
            "path": path,
            "requested_path": requested,
            "offset": offset,
            "length": length
        }),
    );
    if let Some(why) = ctx.path_denied(&path) {
        return Err(ctx.deny_call(&call, &format!("{path} (from {requested}): {why}")));
    }
    ctx.authorize(&call, None, &facts).await?;
    let file = ctx
        .deps
        .files
        .read(&conn, &path, offset, length as usize)
        .await
        .map_err(|e| ToolError::Failed(format!("{}: {e:#}", conn.name)))?;
    if file.data.contains(&0) {
        return Err(ToolError::Invalid(format!(
            "{path} looks like a binary file"
        )));
    }
    let text = String::from_utf8_lossy(&file.data).into_owned();
    let (content, r) = ctx.redact_text(&call, &text);
    let returned = file.data.len() as u64;
    to_value(FileOut {
        host: conn.name.clone(),
        path,
        size: file.size,
        offset,
        bytes_returned: returned,
        truncated: offset + returned < file.size,
        untrusted_content: content,
        redactions: reds(&r),
        notice: NOTICE.into(),
    })
}

/// Build the shell line for `search_logs`. Everything agent-supplied is validated or quoted.
pub fn log_command(a: &SearchLogsArgs, lines: u32) -> Result<String, ToolError> {
    let pattern = a.pattern.as_deref().filter(|p| !p.is_empty());
    if let Some(p) = pattern
        && (p.len() > 512 || p.contains('\0'))
    {
        return Err(ToolError::Invalid("pattern too long".into()));
    }
    if a.source.starts_with('/') {
        if a.since.is_some() {
            return Err(ToolError::Invalid(
                "since only works for journal sources".into(),
            ));
        }
        let path = normalize_path(&a.source)
            .map_err(|e| ToolError::Invalid(format!("{}: {e}", a.source)))?;
        return Ok(match pattern {
            Some(p) => format!(
                "grep -E -e {} -- {} | tail -n {lines}",
                sh_quote(p),
                sh_quote(&path)
            ),
            None => format!("tail -n {lines} -- {}", sh_quote(&path)),
        });
    }
    let mut cmd = String::from("journalctl --no-pager -o short-iso");
    if a.source != "journal" {
        if !valid_unit(&a.source) {
            return Err(ToolError::Invalid(format!(
                "source '{}' is neither a unit name, 'journal' nor an absolute path",
                a.source
            )));
        }
        cmd.push_str(&format!(" -u {}", sh_quote(&a.source)));
    }
    if let Some(s) = a.since.as_deref() {
        if !valid_since(s) {
            return Err(ToolError::Invalid(format!("bad since value '{s}'")));
        }
        cmd.push_str(&format!(" --since {}", sh_quote(s)));
    }
    if let Some(p) = pattern {
        cmd.push_str(&format!(" -g {}", sh_quote(p)));
    }
    cmd.push_str(&format!(" -n {lines}"));
    Ok(cmd)
}

pub(super) async fn search_logs(ctx: &Arc<RunContext>, a: SearchLogsArgs) -> ToolResult {
    let lines = a.lines.unwrap_or(LOG_LINES_DEFAULT).clamp(1, LOG_LINES_MAX);
    let cmd = log_command(&a, lines)?;
    let path = a.source.starts_with('/').then(|| a.source.clone());
    let call = ctx.new_call(
        ToolName::SearchLogs,
        Some(&a.host),
        Some(cmd.clone()),
        path.clone(),
        json!({ "host": a.host, "source": a.source, "pattern": a.pattern, "since": a.since, "lines": lines }),
    );
    if let Some(p) = path.as_deref().and_then(|p| normalize_path(p).ok())
        && let Some(why) = ctx.path_denied(&p)
    {
        return Err(ctx.deny_call(&call, &format!("{p}: {why}")));
    }
    let conn = ctx.resolve_host(&a.host).await?;
    let facts = ctx.facts(&conn).await;
    ctx.authorize(&call, None, &facts).await?;
    let mode = RunContext::read_mode(&facts);
    let jailed = mode.is_jailed();
    let ex = ctx
        .exec(&conn, &call, &cmd, mode, ExecLimits::default())
        .await?;
    to_value(exec_out(&conn.name, &ex, jailed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sl(source: &str, pattern: Option<&str>, since: Option<&str>) -> SearchLogsArgs {
        SearchLogsArgs {
            host: "h".into(),
            source: source.into(),
            pattern: pattern.map(str::to_string),
            since: since.map(str::to_string),
            lines: None,
        }
    }

    #[test]
    fn log_commands_are_quoted_and_validated() {
        let c = log_command(&sl("nginx.service", Some("error|502"), Some("2h ago")), 50).unwrap();
        assert_eq!(
            c,
            "journalctl --no-pager -o short-iso -u 'nginx.service' --since '2h ago' -g 'error|502' -n 50"
        );
        let c = log_command(&sl("journal", None, None), 10).unwrap();
        assert_eq!(c, "journalctl --no-pager -o short-iso -n 10");
        let c = log_command(&sl("/var/log/nginx/error.log", Some("it's"), None), 5).unwrap();
        assert_eq!(
            c,
            r"grep -E -e 'it'\''s' -- '/var/log/nginx/error.log' | tail -n 5"
        );
        let c = log_command(&sl("/var/log/syslog", None, None), 5).unwrap();
        assert_eq!(c, "tail -n 5 -- '/var/log/syslog'");
        assert!(log_command(&sl("x; reboot", None, None), 5).is_err());
        assert!(log_command(&sl("nginx", None, Some("1h'; id #")), 5).is_err());
        assert!(log_command(&sl("/var/log/a", None, Some("1h")), 5).is_err());
        assert!(log_command(&sl("/var/../etc/shadow", None, None), 5).is_err());
    }
}
