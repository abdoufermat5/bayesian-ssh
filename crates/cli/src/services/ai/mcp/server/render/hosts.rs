//! Text renderings of inventory and run-status tools.

use crate::services::ai::mcp::util::fence_untrusted;
use crate::services::ai::ui::{self, Style};
use serde_json::Value;

pub(super) fn render_list_hosts(v: &Value) -> String {
    let hosts = v["hosts"].as_array();
    let Some(hosts) = hosts else {
        return "no hosts in scope".to_string();
    };
    if hosts.is_empty() {
        return "no hosts in scope".to_string();
    }
    let mut lines = Vec::new();
    for h in hosts {
        let name = h["name"].as_str().unwrap_or("-");
        let tags: Vec<&str> = h["tags"]
            .as_array()
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut line = if tags.is_empty() {
            name.to_string()
        } else {
            format!("{name} · {}", tags.join(", "))
        };
        if let Some(bastion) = h["bastion"].as_str() {
            line.push_str(&format!(" · via bastion {bastion}"));
        } else if h["via_bastion"].as_bool().unwrap_or(false) {
            line.push_str(" · via bastion");
        }
        lines.push(line);
    }
    lines.join("\n")
}

pub(super) fn render_host_info(v: &Value, tag: &str) -> String {
    let style = Style::plain();
    let name = v["name"].as_str().unwrap_or("-");
    let host = v["host"].as_str().unwrap_or("-");
    let user = v["user"].as_str().unwrap_or("-");
    let port = v["port"].as_u64().unwrap_or(22);
    let jail = v["enforcement"]["jail"].as_str().unwrap_or("none");
    let user_host_port = format!("{user}@{host}:{port}");
    let jail_str = format!("jail: {jail}");
    let mut parts: Vec<&str> = vec![name, &user_host_port, &jail_str];

    let tags: Vec<&str> = v["tags"]
        .as_array()
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let tags_str;
    if !tags.is_empty() {
        tags_str = tags.join(", ");
        parts.push(&tags_str);
    }

    let mut out = style.facts(&parts);

    if let Some(sys) = v.get("untrusted_system").and_then(Value::as_object) {
        let kernel = sys.get("kernel").and_then(Value::as_str).unwrap_or("");
        let uptime = sys.get("uptime").and_then(Value::as_str).unwrap_or("");
        let sys_text = format!("kernel: {kernel}\nuptime: {uptime}");
        out.push('\n');
        out.push_str(&fence_untrusted(name, &sys_text, tag));
    }
    if let Some(err) = v["remote_error"].as_str() {
        out.push_str(&format!("\nremote error: {err}"));
    }
    if let Some(err) = v["probe_error"].as_str() {
        out.push_str(&format!(
            "\nprobe error: {err} (the host was treated as having no jail or certificates for \
             this call)"
        ));
    }
    out
}

pub(super) fn render_status(v: &Value) -> String {
    let style = Style::plain();
    let run_id = v["run_id"].as_str().unwrap_or("-");
    let short_run = ui::short_id(run_id);
    let client = v["client"].as_str().unwrap_or("-");
    let policy = v["policy"].as_str().unwrap_or("-");

    let header = style.facts(&[
        &format!("run {short_run}"),
        client,
        &format!("policy {policy}"),
    ]);
    let mut lines = vec![header];

    if v["stopped"].as_bool().unwrap_or(false) {
        lines.push("Run is stopped.".to_string());
    }
    if v["paused"].as_bool().unwrap_or(false) {
        let why = v["pause_reason"]
            .as_str()
            .unwrap_or("the circuit breaker paused this run");
        lines.push(format!(
            "{why}. Resume it with `bssh ai resume {short_run}` or in the desktop app."
        ));
    }

    if let Some(budgets) = v.get("budgets") {
        let calls_per_min = budgets["calls_per_min"].as_u64().unwrap_or(0);
        let calls_left = budgets["calls_left_this_minute"].as_u64().unwrap_or(0);
        let changes_per_run = budgets["changes_per_run"].as_u64().unwrap_or(0);
        let changes_left = budgets["changes_left"].as_u64().unwrap_or(0);
        lines.push(format!(
            "budgets: {calls_left}/{calls_per_min} calls left this minute · {changes_left}/{changes_per_run} changes left"
        ));
    }

    let pending = v["pending_approvals"].as_array();
    let changes = v["changes"].as_array();
    let access = v["access_grants"].as_array();

    let mut has_items = false;

    if let Some(p_arr) = pending
        && !p_arr.is_empty()
    {
        has_items = true;
        lines.push("pending approvals:".to_string());
        for p in p_arr {
            let req_id = p["request_id"].as_str().unwrap_or("-");
            let short_req = ui::short_id(req_id);
            let summary = p["summary"].as_str().unwrap_or("");
            lines.push(format!("  {short_req}: {summary}"));
        }
    }

    if let Some(c_arr) = changes
        && !c_arr.is_empty()
    {
        has_items = true;
        lines.push("changes:".to_string());
        for c in c_arr {
            let id = c["change_id"].as_str().unwrap_or("-");
            let short_c = ui::short_id(id);
            let state = c["state"].as_str().unwrap_or("");
            let title = c["title"].as_str().unwrap_or("");
            lines.push(format!("  {short_c} · {state} · {title}"));
        }
    }

    if let Some(a_arr) = access
        && !a_arr.is_empty()
    {
        has_items = true;
        lines.push("access grants:".to_string());
        for a in a_arr {
            let exp = a["expires_in_secs"].as_u64().unwrap_or(0);
            let dur = ui::duration_ms(exp * 1000);
            let tags = a["scope"]["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let hosts = a["scope"]["hosts"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            lines.push(format!(
                "  tags: [{tags}], hosts: [{hosts}] (expires in {dur})"
            ));
        }
    }

    if !has_items {
        lines.push("no pending approvals".to_string());
    }

    lines.join("\n")
}
