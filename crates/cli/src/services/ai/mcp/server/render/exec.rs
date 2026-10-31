//! Text renderings of command-running tools.

use crate::services::ai::mcp::util::fence_untrusted;
use crate::services::ai::ui::{self, Style};
use serde_json::Value;

pub(super) fn render_exec_out(host: &str, v: &Value, tag: &str) -> String {
    let style = Style::plain();
    let exit_str = if v["timed_out"].as_bool().unwrap_or(false) {
        "timed out".to_string()
    } else if let Some(code) = v["exit_code"].as_i64() {
        format!("exit {code}")
    } else {
        "no exit code".to_string()
    };
    let duration_ms = v["duration_ms"].as_u64().unwrap_or(0);
    let duration_str = ui::duration_ms(duration_ms);
    let jailed_str = if v["jailed"].as_bool().unwrap_or(false) {
        "read-only jail"
    } else {
        "no jail"
    };
    let redaction_count: usize = v["redactions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r["count"].as_u64().map(|c| c as usize))
                .sum()
        })
        .unwrap_or(0);

    let mut parts: Vec<&str> = vec![host, &exit_str, &duration_str, jailed_str];
    let redact_str;
    if redaction_count > 0 {
        redact_str = format!("{redaction_count} redacted");
        parts.push(&redact_str);
    }
    if v["truncated"].as_bool().unwrap_or(false) {
        parts.push("truncated");
    }

    let mut out = style.facts(&parts);

    let stdout = v["untrusted_output"].as_str().unwrap_or("");
    let stderr = v["untrusted_stderr"].as_str().unwrap_or("");

    if !stdout.is_empty() {
        out.push('\n');
        out.push_str(&fence_untrusted(host, stdout, tag));
    }
    if !stderr.is_empty() {
        out.push('\n');
        out.push_str("stderr:\n");
        out.push_str(&fence_untrusted(host, stderr, tag));
    }

    out
}

pub(super) fn render_service(v: &Value, tag: &str) -> String {
    let style = Style::plain();
    let host = v["host"].as_str().unwrap_or("-");
    let unit = v["unit"].as_str().unwrap_or("");
    let action = v["action"].as_str().unwrap_or("");
    let status = v["status"].as_str().unwrap_or("");
    let healthy = v["healthy"].as_bool();

    let res = &v["result"];
    let exit_str = if res["timed_out"].as_bool().unwrap_or(false) {
        "timed out".to_string()
    } else if let Some(code) = res["exit_code"].as_i64() {
        format!("exit {code}")
    } else {
        "no exit code".to_string()
    };
    let duration_ms = res["duration_ms"].as_u64().unwrap_or(0);
    let duration_str = ui::duration_ms(duration_ms);
    let jailed_str = if res["jailed"].as_bool().unwrap_or(false) {
        "read-only jail"
    } else {
        "no jail"
    };

    let action_unit = format!("{action} {unit}");
    let mut parts: Vec<&str> = vec![host, &action_unit];
    if status == "ok" {
        parts.push("ok");
    } else if healthy == Some(false) {
        parts.push("failed (inactive)");
    } else {
        parts.push("failed");
    }
    parts.push(&exit_str);
    parts.push(&duration_str);
    parts.push(jailed_str);

    let redaction_count: usize = res["redactions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r["count"].as_u64().map(|c| c as usize))
                .sum()
        })
        .unwrap_or(0);
    let redact_str;
    if redaction_count > 0 {
        redact_str = format!("{redaction_count} redacted");
        parts.push(&redact_str);
    }

    let mut out = style.facts(&parts);

    let stdout = res["untrusted_output"].as_str().unwrap_or("");
    let stderr = res["untrusted_stderr"].as_str().unwrap_or("");
    if !stdout.is_empty() {
        out.push('\n');
        out.push_str(&fence_untrusted(host, stdout, tag));
    }
    if !stderr.is_empty() {
        out.push('\n');
        out.push_str("stderr:\n");
        out.push_str(&fence_untrusted(host, stderr, tag));
    }
    out
}

pub(super) fn render_run_many(v: &Value, tag: &str) -> String {
    let results = v["results"].as_array();
    let Some(results) = results else {
        return "no results".to_string();
    };
    if results.is_empty() {
        return "no hosts matched".to_string();
    }
    let mut entries = Vec::new();
    for entry in results {
        let host = entry["host"].as_str().unwrap_or("-");
        let ok = entry["ok"].as_bool().unwrap_or(false);
        if ok {
            if let Some(res) = entry.get("result") {
                entries.push(render_exec_out(host, res, tag));
            } else {
                entries.push(format!("{host} · ok"));
            }
        } else {
            let err = entry["error"].as_str().unwrap_or("failed");
            entries.push(format!("{host} · failed: {err}"));
        }
    }
    entries.join("\n\n")
}

pub(super) fn render_read_file(v: &Value, tag: &str) -> String {
    let style = Style::plain();
    let host = v["host"].as_str().unwrap_or("-");
    let path = v["path"].as_str().unwrap_or("");
    let offset = v["offset"].as_u64().unwrap_or(0);
    let bytes_returned = v["bytes_returned"].as_u64().unwrap_or(0);
    let size = v["size"].as_u64().unwrap_or(0);
    let truncated = v["truncated"].as_bool().unwrap_or(false);

    let window_str = format!("bytes {}..{} of {}", offset, offset + bytes_returned, size);
    let mut parts: Vec<&str> = vec![host, path, &window_str];
    if truncated {
        parts.push("truncated");
    }
    let redaction_count: usize = v["redactions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r["count"].as_u64().map(|c| c as usize))
                .sum()
        })
        .unwrap_or(0);
    let redact_str;
    if redaction_count > 0 {
        redact_str = format!("{redaction_count} redacted");
        parts.push(&redact_str);
    }

    let mut out = style.facts(&parts);
    let content = v["untrusted_content"].as_str().unwrap_or("");
    if !content.is_empty() {
        out.push('\n');
        out.push_str(&fence_untrusted(host, content, tag));
    }
    if truncated {
        out.push_str(&format!(
            "\nNote: file is truncated ({} bytes remaining); use range to read more.",
            size.saturating_sub(offset + bytes_returned)
        ));
    }
    out
}

pub(super) fn render_search_logs(v: &Value, tag: &str) -> String {
    let style = Style::plain();
    let host = v["host"].as_str().unwrap_or("-");
    let exit_str = if v["timed_out"].as_bool().unwrap_or(false) {
        "timed out".to_string()
    } else if let Some(code) = v["exit_code"].as_i64() {
        format!("exit {code}")
    } else {
        "no exit code".to_string()
    };
    let duration_ms = v["duration_ms"].as_u64().unwrap_or(0);
    let duration_str = ui::duration_ms(duration_ms);
    let jailed_str = if v["jailed"].as_bool().unwrap_or(false) {
        "read-only jail"
    } else {
        "no jail"
    };
    let truncated = v["truncated"].as_bool().unwrap_or(false);

    let mut parts: Vec<&str> = vec![host, "logs", &exit_str, &duration_str, jailed_str];
    if truncated {
        parts.push("truncated");
    }
    let redaction_count: usize = v["redactions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r["count"].as_u64().map(|c| c as usize))
                .sum()
        })
        .unwrap_or(0);
    let redact_str;
    if redaction_count > 0 {
        redact_str = format!("{redaction_count} redacted");
        parts.push(&redact_str);
    }

    let mut out = style.facts(&parts);
    let stdout = v["untrusted_output"].as_str().unwrap_or("");
    let stderr = v["untrusted_stderr"].as_str().unwrap_or("");

    if !stdout.is_empty() {
        out.push('\n');
        out.push_str(&fence_untrusted(host, stdout, tag));
    }
    if !stderr.is_empty() {
        out.push('\n');
        out.push_str("stderr:\n");
        out.push_str(&fence_untrusted(host, stderr, tag));
    }
    if truncated {
        out.push_str("\nNote: log output was truncated.");
    }
    out
}
