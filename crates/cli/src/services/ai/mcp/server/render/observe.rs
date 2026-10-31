//! Text rendering of `observe` results and the generic value formatter.

use crate::services::ai::mcp::util::{fence_untrusted, human_bytes};
use crate::services::ai::ui::Style;
use serde_json::Value;

pub(super) fn render_observe(v: &Value, tag: &str) -> String {
    let style = Style::plain();
    let host = v["host"].as_str().unwrap_or("-");
    let what = v["what"].as_str().unwrap_or("");
    let observe_label = format!("observe {what}");

    let mut parts: Vec<&str> = vec![host, &observe_label];
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
    let data_str = format_observe_data(what, &v["untrusted_data"]);
    if !data_str.is_empty() {
        out.push('\n');
        out.push_str(&fence_untrusted(host, &data_str, tag));
    }
    out
}

pub(super) fn format_observe_data(what: &str, data: &Value) -> String {
    match what {
        "disk" => {
            if let Some(fss) = data["filesystems"].as_array() {
                let mut lines = Vec::new();
                for fs in fss {
                    let mount = fs["mount"].as_str().unwrap_or("-");
                    let total = fs["total_bytes"].as_u64().unwrap_or(0);
                    let used = fs["used_bytes"].as_u64().unwrap_or(0);
                    let pct = fs["use_percent"].as_u64().unwrap_or(0);
                    let dev = fs["filesystem"].as_str().unwrap_or("");
                    lines.push(format!(
                        "{mount} · {} used / {} ({pct}%) · {dev}",
                        human_bytes(used),
                        human_bytes(total)
                    ));
                }
                return lines.join("\n");
            }
        }
        "memory" => {
            let mut lines = Vec::new();
            if let Some(mem) = data.get("memory").filter(|v| v.is_object()) {
                let total = mem["total_bytes"].as_u64().unwrap_or(0);
                let used = mem["used_bytes"].as_u64().unwrap_or(0);
                let avail = mem["available_bytes"].as_u64().unwrap_or(0);
                lines.push(format!(
                    "Mem: {} used / {} total ({} available)",
                    human_bytes(used),
                    human_bytes(total),
                    human_bytes(avail)
                ));
            }
            if let Some(swap) = data.get("swap").filter(|v| v.is_object()) {
                let total = swap["total_bytes"].as_u64().unwrap_or(0);
                let used = swap["used_bytes"].as_u64().unwrap_or(0);
                lines.push(format!(
                    "Swap: {} used / {} total",
                    human_bytes(used),
                    human_bytes(total)
                ));
            }
            if !lines.is_empty() {
                return lines.join("\n");
            }
        }
        "services" => {
            if let Some(units) = data["units"].as_array() {
                if units.is_empty() {
                    return "no services found".to_string();
                }
                let mut lines = Vec::new();
                for u in units {
                    let unit = u["unit"].as_str().unwrap_or("");
                    let active = u["active"].as_str().unwrap_or("");
                    let sub = u["sub"].as_str().unwrap_or("");
                    let desc = u["description"].as_str().unwrap_or("");
                    lines.push(format!("{unit} · {active}/{sub} · {desc}"));
                }
                return lines.join("\n");
            }
        }
        "failed_units" => {
            if let Some(units) = data["units"].as_array() {
                if units.is_empty() {
                    return "no failed units".to_string();
                }
                let mut lines = Vec::new();
                for u in units {
                    let unit = u["unit"].as_str().unwrap_or("");
                    let sub = u["sub"].as_str().unwrap_or("");
                    let desc = u["description"].as_str().unwrap_or("");
                    lines.push(format!("{unit} · {sub} · {desc}"));
                }
                return lines.join("\n");
            }
        }
        "ports" => {
            if let Some(ports) = data.as_array() {
                let mut lines = Vec::new();
                for p in ports {
                    let proto = p["proto"].as_str().unwrap_or("");
                    let addr = p["local_address"].as_str().unwrap_or("");
                    let port = p["local_port"].as_u64().unwrap_or(0);
                    let proc = p["process"].as_str().unwrap_or("-");
                    let pid = p["pid"].as_u64().unwrap_or(0);
                    lines.push(format!("{proto} {addr}:{port} ({proc} pid {pid})"));
                }
                return lines.join("\n");
            }
        }
        "processes" => {
            if let Some(procs) = data.as_array() {
                let mut lines = Vec::new();
                for p in procs {
                    let pid = p["pid"].as_u64().unwrap_or(0);
                    let user = p["user"].as_str().unwrap_or("");
                    let cpu = p["cpu_percent"].as_f64().unwrap_or(0.0);
                    let mem = p["mem_percent"].as_f64().unwrap_or(0.0);
                    let comm = p["comm"].as_str().unwrap_or("");
                    lines.push(format!(
                        "{pid:>5} {user:<10} {cpu:>5.1}% CPU {mem:>5.1}% MEM {comm}"
                    ));
                }
                return lines.join("\n");
            }
        }
        "containers" => {
            if let Some(conts) = data.as_array() {
                let mut lines = Vec::new();
                for c in conts {
                    let names = c["names"].as_str().unwrap_or("");
                    let image = c["image"].as_str().unwrap_or("");
                    let status = c["status"].as_str().unwrap_or("");
                    lines.push(format!("{names} · {image} · {status}"));
                }
                return lines.join("\n");
            }
        }
        "packages" => {
            if let Some(pkgs) = data.as_array() {
                let mut lines = Vec::new();
                for p in pkgs {
                    let pkg = p["package"].as_str().unwrap_or("");
                    let ver = p["version"].as_str().unwrap_or("");
                    lines.push(format!("{pkg} {ver}"));
                }
                return lines.join("\n");
            }
        }
        _ => {}
    }
    format_clean_value(data)
}

pub(super) fn format_clean_value(val: &Value) -> String {
    match val {
        Value::Object(map) => {
            let mut lines = Vec::new();
            for (k, v) in map {
                match v {
                    Value::String(s) => lines.push(format!("{k}: {s}")),
                    Value::Number(n) => lines.push(format!("{k}: {n}")),
                    Value::Bool(b) => lines.push(format!("{k}: {b}")),
                    Value::Array(arr) => {
                        lines.push(format!("{k}: ({} items)", arr.len()));
                        for item in arr {
                            lines.push(format!("  {}", format_clean_value(item)));
                        }
                    }
                    Value::Object(_) => {
                        lines.push(format!("{k}:"));
                        for l in format_clean_value(v).lines() {
                            lines.push(format!("  {l}"));
                        }
                    }
                    Value::Null => lines.push(format!("{k}: none")),
                }
            }
            lines.join("\n")
        }
        Value::Array(arr) => arr
            .iter()
            .map(format_clean_value)
            .collect::<Vec<_>>()
            .join("\n"),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "none".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::format_observe_data;
    use crate::services::ai::observe::parse_memory;

    #[test]
    fn memory_render_reads_what_the_parser_emits() {
        let with_swap = parse_memory(
            "               total        used        free      shared  buff/cache   available\n\
             Mem:     10426716160  1096122368  7000039424      344064  2674077696  9330593792\n\
             Swap:      511700992   504987648     6713344\n",
        )
        .unwrap();
        let text = format_observe_data("memory", &with_swap);
        assert!(text.contains("Mem: 1.0 GB used / 9.7 GB total"), "{text}");
        assert!(text.contains("Swap:"), "{text}");

        // No swap line: `swap` is null and must not render as "0 B / 0 B".
        let no_swap = parse_memory(
            "Mem:     8000000000  2000000000  3000000000  0  3000000000  5500000000\n",
        )
        .unwrap();
        let text = format_observe_data("memory", &no_swap);
        assert!(text.starts_with("Mem:"), "{text}");
        assert!(!text.contains("Swap"), "{text}");
    }
}
