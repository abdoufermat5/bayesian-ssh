//! [`DefaultPolicyEngine`]: the evaluation order documented in the parent module.

use super::budget::parse_duration;
use super::candidates::path_candidates;
use super::commands::matches_deny_fallback;
use super::deny_paths::{deny_path_hit, path_matches_any, PathHit};
use super::grants::{grant_matches, needs_host};
use super::scope::{ignored_tag_note, in_scope, TagTrust};
use super::tier::tier_for;
use crate::services::ai::store::{AiStore, CounterKind};
use crate::services::ai::types::{
    Analysis, Decision, HostFacts, JailKind, Policy, PolicyEngine, Profile, ToolCall, ToolName,
};
use std::sync::Arc;
use std::time::Duration;

pub struct DefaultPolicyEngine {
    store: Arc<AiStore>,
}

impl DefaultPolicyEngine {
    /// `store` gives access to active grants and budget counters.
    pub fn new(store: Arc<AiStore>) -> Self {
        Self { store }
    }

    fn note(&self, run_id: &str, kind: CounterKind) {
        if let Err(e) = self.store.note(run_id, kind) {
            tracing::warn!("policy: could not update counters: {e}");
        }
    }

    fn check_budgets(&self, policy: &Policy, call: &ToolCall) -> Option<Decision> {
        let b = &policy.budgets;
        let run = &call.run_id;
        if b.calls_per_min > 0 {
            let n = self
                .store
                .count_within(run, CounterKind::Call, Duration::from_secs(60))
                .unwrap_or(0);
            if n > b.calls_per_min {
                return Some(Decision::Deny {
                    reason: format!(
                        "budget.calls_per_min: {} calls in the last minute, limit is {}",
                        n - 1,
                        b.calls_per_min
                    ),
                });
            }
        }
        if b.changes_per_run > 0 && call.tool.is_write() {
            let n = self.store.changes_in_run(run).unwrap_or(0);
            if n >= b.changes_per_run {
                return Some(Decision::Deny {
                    reason: format!(
                        "budget.changes_per_run: {n} changes already made, limit is {}",
                        b.changes_per_run
                    ),
                });
            }
        }
        if let Some(ttl) = parse_duration(&b.run_ttl) {
            if let Ok(Some(rec)) = self.store.run(run) {
                if let Ok(start) = chrono::DateTime::parse_from_rfc3339(&rec.started) {
                    let age = chrono::Utc::now().signed_duration_since(start);
                    if age.to_std().map(|a| a > ttl).unwrap_or(false) {
                        return Some(Decision::Deny {
                            reason: format!("budget.run_ttl: the run is older than {}", b.run_ttl),
                        });
                    }
                }
            }
        }
        None
    }

    fn decide_inner(
        &self,
        policy: &Policy,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        host: &HostFacts,
    ) -> Decision {
        let host_name = if host.name.is_empty() {
            call.host.clone().unwrap_or_default()
        } else {
            host.name.clone()
        };
        let deny = |reason: String| Decision::Deny { reason };

        // 1. scope
        if host_name.is_empty() {
            if needs_host(call.tool) {
                return deny("target: the call names no host".into());
            }
        } else if !in_scope(
            &policy.scope,
            &host_name,
            &host.tags,
            TagTrust::from_flag(host.tags_trusted),
        ) {
            self.note(&call.run_id, CounterKind::Probe);
            let mut reason = format!("scope: host '{host_name}' is not in this policy's scope");
            if let Some(note) = ignored_tag_note(&policy.scope.tags, host) {
                reason = format!("{reason} ({note})");
            }
            return deny(reason);
        }

        // 2. budgets
        if let Some(d) = self.check_budgets(policy, call) {
            return d;
        }

        // 3. profile
        if policy.profile == Profile::Observe && call.tool.is_write() {
            return deny("profile.observe: the observe profile cannot change anything".into());
        }

        // 4. deny_paths: the tool's path, then every path the command names as written
        // (arguments, input and output redirections, `--opt=`/attached-option values,
        // `~`/`$HOME` paths and arguments relative to a literal `cd`)
        if let Some(p) = &call.path {
            if let Some(pat) = path_matches_any(&policy.deny_paths, p) {
                return deny(format!("deny_paths: '{p}' matches '{pat}'"));
            }
        }
        let (candidates, runtime_path) = path_candidates(analysis);
        let mut unsure =
            runtime_path.map(|p| format!("deny_paths: '{p}' is a path only known at run time"));
        for c in &candidates {
            match deny_path_hit(&policy.deny_paths, c) {
                Some(PathHit::Deny(pat)) => {
                    return deny(format!("deny_paths: '{}' matches '{pat}'", c.text));
                }
                Some(PathHit::Unsure(pat)) => {
                    unsure.get_or_insert_with(|| {
                        format!(
                            "deny_paths: '{}' could expand to a path matching '{pat}'",
                            c.text
                        )
                    });
                }
                None => {}
            }
        }

        // 5. deny patterns: on the parsed commands, so quoting, wrappers and `/bin/rm` can't
        // dodge them. The text matcher (whose `*` crosses `/`) only adds to that when bssh
        // couldn't fully parse the call; on a clean parse it would hard-deny
        // `rm -rf /var/www/cache` through `rm -rf /*`.
        let ast_hit = analysis.and_then(|a| {
            policy
                .deny
                .iter()
                .find(|p| crate::services::ai::analyzer::matches_deny(p, a))
                .cloned()
        });
        let hit = match analysis {
            Some(a) if a.unknown_reason.is_none() => ast_hit,
            _ => ast_hit.or_else(|| matches_deny_fallback(&policy.deny, call, analysis)),
        };
        if let Some(pat) = hit {
            return deny(format!("deny: the command matches '{pat}'"));
        }

        // Everything that asks goes through here: it picks the tier and says when a tag that
        // would have allowed in-client approval was ignored because its source is not trusted.
        let ask = |reason: String| {
            let reason = match ignored_tag_note(&policy.approval.in_client, host) {
                Some(note) => format!("{reason}; {note}"),
                None => reason,
            };
            Decision::Ask {
                tier: tier_for(policy, call, analysis, host, &host_name),
                reason,
            }
        };

        // A glob or a run-time path could still name a deny path, and the analyzer can't
        // settle it: never allow those automatically, whatever the grants say.
        if let Some(reason) = unsure {
            return ask(reason);
        }

        // 6. write_paths
        if call.tool == ToolName::EditFile && !policy.write_paths.is_empty() {
            match &call.path {
                Some(p) if path_matches_any(&policy.write_paths, p).is_some() => {}
                Some(p) => {
                    return deny(format!("write_paths: '{p}' is outside the writable paths"))
                }
                None => return deny("write_paths: edit_file without a path".into()),
            }
        }

        // 7. grants
        match self.store.active_grants(Some(&call.run_id)) {
            Ok(grants) => {
                if let Some(g) = grants.iter().find(|g| grant_matches(g, call, &host_name)) {
                    return Decision::Allow {
                        reason: format!("grant: {} until {}", g.grant_id, g.expires_at),
                    };
                }
            }
            Err(e) => tracing::warn!("policy: could not read grants: {e}"),
        }

        // 8. reads
        match call.tool {
            ToolName::ListHosts | ToolName::HostInfo | ToolName::Status => {
                return Decision::Allow {
                    reason: "read: metadata tools are always allowed in scope".into(),
                }
            }
            ToolName::Observe | ToolName::SearchLogs => {
                return Decision::Allow {
                    reason: "read: observe tools run read-only commands".into(),
                }
            }
            ToolName::ReadFile => {
                return match &call.path {
                    Some(p) if path_matches_any(&policy.read_paths, p).is_some() => {
                        Decision::Allow {
                            reason: format!("read_paths: '{p}' is readable"),
                        }
                    }
                    Some(p) => ask(format!("read_paths: '{p}' is outside the readable paths")),
                    None => deny("target: read_file without a path".into()),
                }
            }
            ToolName::RunReadonly | ToolName::RunMany => {
                // A jail stops writes, but deny paths are only enforced when it runs as the
                // low-privilege agent principal: Landlock can only allow, and bubblewrap masks
                // literal deny paths only (not globs such as `**/.ssh/**`).
                if host.enforcement.jail != JailKind::None && host.enforcement.certificates {
                    return Decision::Allow {
                        reason: format!(
                            "jail: the host enforces a {} read-only jail and it runs as the \
                             low-privilege agent principal",
                            host.enforcement.jail.as_str()
                        ),
                    };
                }
                let boundary = match host.enforcement.jail {
                    JailKind::None => "the host has no jail",
                    _ => "the host's jail cannot enforce deny_paths",
                };
                return match analysis {
                    Some(a) if a.is_read_only() => Decision::Allow {
                        reason: "analyzer: every command is read-only".into(),
                    },
                    Some(a) => ask(format!(
                        "analyzer: not provably read-only ({}) and {boundary}",
                        describe_tags(a)
                    )),
                    None => ask(format!("analyzer: no analysis and {boundary}")),
                };
            }
            ToolName::RequestAccess => {
                return Decision::Ask {
                    tier: policy.approval.default,
                    reason: "request_access: wider scope always needs a human".into(),
                }
            }
            _ => {}
        }

        // 9. writes
        ask(format!(
            "write: {} changes the host and needs approval",
            super::super::mcp::tools::name_str(call.tool)
        ))
    }
}

fn describe_tags(a: &Analysis) -> String {
    if let Some(r) = &a.unknown_reason {
        return format!("unknown: {r}");
    }
    a.tags
        .iter()
        .map(|t| format!("{t:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}

impl PolicyEngine for DefaultPolicyEngine {
    fn decide(
        &self,
        policy: &Policy,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        host: &HostFacts,
    ) -> Decision {
        self.note(&call.run_id, CounterKind::Call);
        let d = self.decide_inner(policy, call, analysis, host);
        if matches!(d, Decision::Deny { .. }) {
            self.note(&call.run_id, CounterKind::Denial);
        }
        d
    }
}
