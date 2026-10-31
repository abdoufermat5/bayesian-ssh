//! Sudoers lines to suggest for a policy; bssh never writes them.

use crate::services::ai::jail::{self};
use crate::services::ai::types::{Policy, Profile};
use serde::Serialize;

use super::scripts::AGENT_USER;

/// A line the admin may add to sudoers, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SudoersSuggestion {
    pub line: String,
    pub why: String,
}

/// Config directories whose services a policy may change, with the service name and an
/// optional config test (all with fixed arguments: no wildcards).
const SERVICE_HINTS: &[(&str, &str, Option<&str>)] = &[
    ("/etc/nginx", "nginx", Some("/usr/sbin/nginx -t")),
    (
        "/etc/apache2",
        "apache2",
        Some("/usr/sbin/apache2ctl configtest"),
    ),
    ("/etc/httpd", "httpd", Some("/usr/sbin/httpd -t")),
    (
        "/etc/haproxy",
        "haproxy",
        Some("/usr/sbin/haproxy -c -f /etc/haproxy/haproxy.cfg"),
    ),
    ("/etc/postgresql", "postgresql", None),
    ("/etc/redis", "redis-server", None),
    ("/etc/prometheus", "prometheus", None),
    ("/etc/grafana", "grafana-server", None),
];

/// Narrow sudoers lines for the services behind the policy's write paths. Never `ALL`:
/// each command is spelled out with its arguments. Nothing is installed by bssh.
pub fn sudoers_suggestions(policy: &Policy) -> Vec<SudoersSuggestion> {
    if policy.profile == Profile::Observe && policy.write_paths.is_empty() {
        return Vec::new();
    }
    let roots: Vec<String> = policy
        .write_paths
        .iter()
        .filter_map(|p| jail::read_root(p))
        .collect();
    let mut out = Vec::new();
    for (dir, service, test) in SERVICE_HINTS {
        if !roots.iter().any(|r| {
            r == dir
                || r.starts_with(&format!("{dir}/"))
                || (r != "/" && dir.starts_with(&format!("{r}/")))
        }) {
            continue;
        }
        let mut cmds = vec![
            format!("/usr/bin/systemctl reload {service}"),
            format!("/usr/bin/systemctl restart {service}"),
        ];
        if let Some(t) = test {
            cmds.push((*t).to_string());
        }
        out.push(SudoersSuggestion {
            line: format!("{AGENT_USER} ALL=(root) NOPASSWD: {}", cmds.join(", ")),
            why: format!("the policy may change files under {dir}: allow reloading {service} and testing its config"),
        });
    }
    if roots
        .iter()
        .any(|r| r == "/etc/systemd/system" || r.starts_with("/etc/systemd/system/"))
    {
        out.push(SudoersSuggestion {
            line: format!("{AGENT_USER} ALL=(root) NOPASSWD: /usr/bin/systemctl daemon-reload"),
            why: "the policy may change unit files under /etc/systemd/system".into(),
        });
    }
    out
}
