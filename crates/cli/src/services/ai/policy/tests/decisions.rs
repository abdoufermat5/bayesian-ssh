use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::ai::policy::scope::{TagTrust, in_scope};
use crate::services::ai::policy::{
    ALL_HOSTS, DefaultPolicyEngine, PROBE_LIMIT, builtin_profile, circuit_breaker,
    connection_facts, host_in_scope, resolve, tags_trusted,
};
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    Analysis, ApprovalRules, Decision, Enforcement, Grant, HostFacts, JailKind, Policy,
    PolicyEngine, Profile, RiskTag, Scope, SimpleCommand, Tier, ToolCall, ToolName,
};
use chrono::Utc;
use std::collections::BTreeSet;
use std::sync::Arc;

struct Fixture {
    _tmp: tempfile::TempDir,
    store: Arc<AiStore>,
    engine: DefaultPolicyEngine,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
    Fixture {
        engine: DefaultPolicyEngine::new(store.clone()),
        store,
        _tmp: tmp,
    }
}

fn operate() -> Policy {
    let mut p = builtin_profile(Profile::Operate);
    p.scope.tags = vec!["staging".into(), "prod".into()];
    p
}

fn host(name: &str, tags: &[&str], jail: JailKind) -> HostFacts {
    HostFacts {
        name: name.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        tags_trusted: true,
        tag_source: None,
        enforcement: Enforcement {
            jail,
            ..Default::default()
        },
        ..HostFacts::default()
    }
}

/// The same host, but its tags come from a source `ai.trusted_tag_sources` doesn't name.
fn untrusted_host(name: &str, tags: &[&str], source: &str) -> HostFacts {
    HostFacts {
        tags_trusted: false,
        tag_source: Some(source.into()),
        ..host(name, tags, JailKind::None)
    }
}

/// The same host, but it trusts the bssh CA and has the `bssh-agent` user.
fn host_trusting_ca(name: &str, tags: &[&str], jail: JailKind) -> HostFacts {
    HostFacts {
        enforcement: Enforcement {
            jail,
            certificates: true,
            agent_user: true,
        },
        ..host(name, tags, jail)
    }
}

fn call(tool: ToolName, host: Option<&str>) -> ToolCall {
    ToolCall {
        run_id: "run1".into(),
        call_id: "c1".into(),
        tool,
        host: host.map(String::from),
        command: None,
        path: None,
        args: serde_json::Value::Null,
    }
}

fn cmd_call(tool: ToolName, h: &str, command: &str) -> ToolCall {
    ToolCall {
        command: Some(command.into()),
        ..call(tool, Some(h))
    }
}

fn path_call(tool: ToolName, h: &str, path: &str) -> ToolCall {
    ToolCall {
        path: Some(path.into()),
        ..call(tool, Some(h))
    }
}

fn analysis(program: &str, args: &[&str], tags: &[RiskTag]) -> Analysis {
    let tags: BTreeSet<RiskTag> = tags.iter().copied().collect();
    Analysis {
        commands: vec![SimpleCommand {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            wrappers: vec![],
            writes_to: vec![],
            reads_from: vec![],
            glob_args: vec![],
            cwd: None,
            tags: tags.clone(),
            rule: None,
        }],
        tags,
        unknown_reason: None,
    }
}

fn unknown_analysis() -> Analysis {
    Analysis {
        commands: vec![],
        tags: [RiskTag::Unknown].into_iter().collect(),
        unknown_reason: Some("eval".into()),
    }
}

fn is_allow(d: &Decision) -> bool {
    matches!(d, Decision::Allow { .. })
}
fn is_deny(d: &Decision) -> bool {
    matches!(d, Decision::Deny { .. })
}
pub(super) fn reason(d: &Decision) -> &str {
    match d {
        Decision::Allow { reason } | Decision::Deny { reason } => reason,
        Decision::Ask { reason, .. } => reason,
    }
}
fn tier(d: &Decision) -> Tier {
    match d {
        Decision::Ask { tier, .. } => *tier,
        other => panic!("expected ask, got {other:?}"),
    }
}

// ---- decision table

#[test]
fn decision_table() {
    use ToolName::*;
    let stg = host("web-1", &["staging"], JailKind::None);
    let stg_jail = host("web-1", &["staging"], JailKind::Landlock);
    let stg_bwrap = host("web-1", &["staging"], JailKind::Bubblewrap);
    let stg_agent = host_trusting_ca("web-1", &["staging"], JailKind::Landlock);
    let other = host("db-9", &["lab"], JailKind::None);
    let ro = analysis("cat", &["/etc/hosts"], &[RiskTag::Read]);
    let wr = analysis(
        "systemctl",
        &["restart", "nginx"],
        &[RiskTag::ServiceChange],
    );

    // (label, policy, call, analysis, host, expected: 'A' allow, 'K' ask, 'D' deny, rule fragment)
    type Case<'a> = (
        &'a str,
        Policy,
        ToolCall,
        Option<Analysis>,
        HostFacts,
        char,
        &'a str,
    );
    let mut cases: Vec<Case> = vec![
        (
            "list_hosts allowed",
            operate(),
            call(ListHosts, None),
            None,
            stg.clone(),
            'A',
            "read",
        ),
        (
            "status allowed",
            operate(),
            call(Status, None),
            None,
            stg.clone(),
            'A',
            "read",
        ),
        (
            "host_info in scope",
            operate(),
            call(HostInfo, Some("web-1")),
            None,
            stg.clone(),
            'A',
            "read",
        ),
        (
            "host_info out of scope",
            operate(),
            call(HostInfo, Some("db-9")),
            None,
            other.clone(),
            'D',
            "scope",
        ),
        (
            "observe in scope",
            operate(),
            call(Observe, Some("web-1")),
            None,
            stg.clone(),
            'A',
            "read",
        ),
        (
            "observe out of scope",
            operate(),
            call(Observe, Some("db-9")),
            None,
            other.clone(),
            'D',
            "scope",
        ),
        (
            "search_logs",
            operate(),
            call(SearchLogs, Some("web-1")),
            None,
            stg.clone(),
            'A',
            "read",
        ),
        (
            "observe without host",
            operate(),
            call(Observe, None),
            None,
            HostFacts::default(),
            'D',
            "target",
        ),
        (
            "read_file under read_paths",
            operate(),
            path_call(ReadFile, "web-1", "/var/log/nginx/error.log"),
            None,
            stg.clone(),
            'A',
            "read_paths",
        ),
        (
            "read_file outside read_paths asks",
            operate(),
            path_call(ReadFile, "web-1", "/home/bob/notes"),
            None,
            stg.clone(),
            'K',
            "read_paths",
        ),
        (
            "read_file dotdot escape asks",
            operate(),
            path_call(ReadFile, "web-1", "/var/log/../../home/bob/x"),
            None,
            stg.clone(),
            'K',
            "read_paths",
        ),
        (
            "read_file shadow denied",
            operate(),
            path_call(ReadFile, "web-1", "/etc/shadow"),
            None,
            stg.clone(),
            'D',
            "deny_paths",
        ),
        (
            "read_file dotdot to shadow denied",
            operate(),
            path_call(ReadFile, "web-1", "/var/log/../../etc/shadow"),
            None,
            stg.clone(),
            'D',
            "deny_paths",
        ),
        (
            "read_file ssh key denied",
            operate(),
            path_call(ReadFile, "web-1", "/home/bob/.ssh/id_ed25519"),
            None,
            stg.clone(),
            'D',
            "deny_paths",
        ),
        (
            "read_file .env denied",
            operate(),
            path_call(ReadFile, "web-1", "/srv/app/.env"),
            None,
            stg.clone(),
            'D',
            "deny_paths",
        ),
        (
            "read_file .key denied",
            operate(),
            path_call(ReadFile, "web-1", "/etc/ssl/site.key"),
            None,
            stg.clone(),
            'D',
            "deny_paths",
        ),
        (
            "read_file without path denied",
            operate(),
            call(ReadFile, Some("web-1")),
            None,
            stg.clone(),
            'D',
            "target",
        ),
        (
            "run_readonly jailed by bubblewrap as the login user",
            operate(),
            cmd_call(RunReadonly, "web-1", "rm x"),
            Some(wr.clone()),
            stg_bwrap.clone(),
            'K',
            "analyzer",
        ),
        (
            "run_readonly jailed as the agent principal",
            operate(),
            cmd_call(RunReadonly, "web-1", "rm x"),
            Some(wr.clone()),
            stg_agent.clone(),
            'A',
            "jail",
        ),
        (
            "run_readonly landlock as the login user asks",
            operate(),
            cmd_call(RunReadonly, "web-1", "rm x"),
            Some(wr.clone()),
            stg_jail.clone(),
            'K',
            "analyzer",
        ),
        (
            "run_readonly read-only analysis",
            operate(),
            cmd_call(RunReadonly, "web-1", "cat /etc/hosts"),
            Some(ro.clone()),
            stg.clone(),
            'A',
            "analyzer",
        ),
        (
            "run_readonly write analysis asks",
            operate(),
            cmd_call(RunReadonly, "web-1", "systemctl restart nginx"),
            Some(wr.clone()),
            stg.clone(),
            'K',
            "analyzer",
        ),
        (
            "run_readonly unknown analysis asks",
            operate(),
            cmd_call(RunReadonly, "web-1", "eval x"),
            Some(unknown_analysis()),
            stg.clone(),
            'K',
            "analyzer",
        ),
        (
            "run_readonly without analysis asks",
            operate(),
            cmd_call(RunReadonly, "web-1", "ls"),
            None,
            stg.clone(),
            'K',
            "analyzer",
        ),
        (
            "run_many jailed as the agent principal",
            operate(),
            cmd_call(RunMany, "web-1", "uptime"),
            None,
            stg_agent.clone(),
            'A',
            "jail",
        ),
        (
            "run_many read-only",
            operate(),
            cmd_call(RunMany, "web-1", "uptime"),
            Some(analysis("uptime", &[], &[RiskTag::Read])),
            stg.clone(),
            'A',
            "analyzer",
        ),
        (
            "run_many out of scope",
            operate(),
            cmd_call(RunMany, "db-9", "uptime"),
            None,
            other.clone(),
            'D',
            "scope",
        ),
        (
            "jailed run_readonly deny pattern wins",
            operate(),
            cmd_call(RunReadonly, "web-1", "rm -rf /"),
            Some(analysis("rm", &["-rf", "/"], &[RiskTag::Destructive])),
            stg_jail.clone(),
            'D',
            "deny",
        ),
        (
            "jailed run_readonly reading shadow denied",
            operate(),
            cmd_call(RunReadonly, "web-1", "cat /etc/shadow"),
            Some(analysis("cat", &["/etc/shadow"], &[RiskTag::Read])),
            stg_jail.clone(),
            'D',
            "deny_paths",
        ),
        (
            "run asks",
            operate(),
            cmd_call(Run, "web-1", "systemctl restart nginx"),
            Some(wr.clone()),
            stg.clone(),
            'K',
            "write",
        ),
        (
            "run observe denied",
            {
                let mut p = operate();
                p.profile = Profile::Observe;
                p
            },
            cmd_call(Run, "web-1", "ls"),
            Some(ro.clone()),
            stg.clone(),
            'D',
            "profile.observe",
        ),
        (
            "edit_file observe denied",
            {
                let mut p = operate();
                p.profile = Profile::Observe;
                p
            },
            path_call(EditFile, "web-1", "/etc/nginx/a.conf"),
            None,
            stg.clone(),
            'D',
            "profile.observe",
        ),
        (
            "service observe denied",
            {
                let mut p = operate();
                p.profile = Profile::Observe;
                p
            },
            call(Service, Some("web-1")),
            None,
            stg.clone(),
            'D',
            "profile.observe",
        ),
        (
            "apply_change observe denied",
            {
                let mut p = operate();
                p.profile = Profile::Observe;
                p
            },
            call(ApplyChange, None),
            None,
            HostFacts::default(),
            'D',
            "profile.observe",
        ),
        (
            "propose_change operate asks",
            operate(),
            call(ProposeChange, None),
            None,
            HostFacts::default(),
            'K',
            "write",
        ),
        (
            "undo operate asks",
            operate(),
            call(Undo, None),
            None,
            HostFacts::default(),
            'K',
            "write",
        ),
        (
            "run deny pattern",
            operate(),
            cmd_call(Run, "web-1", "rm -rf /"),
            Some(analysis("rm", &["-rf", "/"], &[RiskTag::Destructive])),
            stg.clone(),
            'D',
            "deny",
        ),
        (
            "run deny pattern with trailing flag",
            operate(),
            cmd_call(Run, "web-1", "rm -rf / --no-preserve-root"),
            Some(analysis(
                "rm",
                &["-rf", "/", "--no-preserve-root"],
                &[RiskTag::Destructive],
            )),
            stg.clone(),
            'D',
            "deny",
        ),
        (
            "run deny pattern via full path",
            operate(),
            cmd_call(Run, "web-1", "/bin/mkfs /dev/sda"),
            Some(analysis(
                "/bin/mkfs",
                &["/dev/sda"],
                &[RiskTag::Destructive],
            )),
            stg.clone(),
            'D',
            "deny",
        ),
        (
            "run deny pattern raw text, no analysis",
            operate(),
            cmd_call(Run, "web-1", "ls; rm -rf /"),
            None,
            stg.clone(),
            'D',
            "deny",
        ),
        (
            "rm -rf on a deep path asks, rm -rf /* only covers the top level",
            operate(),
            cmd_call(Run, "web-1", "sudo bash -c 'rm -rf /var/www/cache'"),
            Some(analysis(
                "rm",
                &["-rf", "/var/www/cache"],
                &[RiskTag::Destructive, RiskTag::Privilege],
            )),
            stg.clone(),
            'K',
            "write",
        ),
        (
            "rm -rf on a top-level directory is denied",
            operate(),
            cmd_call(Run, "web-1", "rm -rf /var"),
            Some(analysis("rm", &["-rf", "/var"], &[RiskTag::Destructive])),
            stg.clone(),
            'D',
            "rm -rf /*",
        ),
        (
            "run deny glob dd",
            operate(),
            cmd_call(Run, "web-1", "dd of=/dev/sda if=x"),
            Some(analysis(
                "dd",
                &["of=/dev/sda", "if=x"],
                &[RiskTag::Destructive],
            )),
            stg.clone(),
            'D',
            "deny",
        ),
        (
            "run write to deny_path via redirect",
            operate(),
            cmd_call(Run, "web-1", "echo x > /etc/shadow"),
            Some({
                let mut a = analysis("echo", &["x"], &[RiskTag::Write]);
                a.commands[0].writes_to.push("/etc/shadow".into());
                a
            }),
            stg.clone(),
            'D',
            "deny_paths",
        ),
        (
            "run touching ssh dir denied",
            operate(),
            cmd_call(Run, "web-1", "cat /root/.ssh/authorized_keys"),
            Some(analysis(
                "cat",
                &["/root/.ssh/authorized_keys"],
                &[RiskTag::Read],
            )),
            stg.clone(),
            'D',
            "deny_paths",
        ),
        (
            "edit_file default tier",
            operate(),
            path_call(EditFile, "web-1", "/etc/nginx/a.conf"),
            None,
            stg.clone(),
            'K',
            "write",
        ),
        (
            "edit_file outside write_paths denied",
            {
                let mut p = operate();
                p.write_paths = vec!["/etc/nginx/**".into()];
                p
            },
            path_call(EditFile, "web-1", "/etc/passwd"),
            None,
            stg.clone(),
            'D',
            "write_paths",
        ),
        (
            "edit_file inside write_paths asks",
            {
                let mut p = operate();
                p.write_paths = vec!["/etc/nginx/**".into()];
                p
            },
            path_call(EditFile, "web-1", "/etc/nginx/conf.d/a.conf"),
            None,
            stg.clone(),
            'K',
            "write",
        ),
        (
            "edit_file dotdot out of write_paths denied",
            {
                let mut p = operate();
                p.write_paths = vec!["/etc/nginx/**".into()];
                p
            },
            path_call(EditFile, "web-1", "/etc/nginx/../passwd"),
            None,
            stg.clone(),
            'D',
            "write_paths",
        ),
        (
            "request_access asks even in observe",
            {
                let mut p = operate();
                p.profile = Profile::Observe;
                p
            },
            call(RequestAccess, None),
            None,
            HostFacts::default(),
            'K',
            "request_access",
        ),
        (
            "empty scope means no host",
            {
                let mut p = operate();
                p.scope = Scope::default();
                p
            },
            call(Observe, Some("web-1")),
            None,
            stg.clone(),
            'D',
            "scope",
        ),
        (
            "scope by host name",
            {
                let mut p = operate();
                p.scope = Scope {
                    tags: vec![],
                    hosts: vec!["web-*".into()],
                };
                p
            },
            call(Observe, Some("web-1")),
            None,
            stg.clone(),
            'A',
            "read",
        ),
        (
            "scope wildcard tag",
            {
                let mut p = operate();
                p.scope.tags = vec![ALL_HOSTS.into()];
                p
            },
            call(Observe, Some("db-9")),
            None,
            other.clone(),
            'A',
            "read",
        ),
    ];
    // drop the unused moves
    let _ = &mut cases;

    // One store for all cases; each case gets its own run id so counters don't mix.
    let fx = fixture();
    for (n, (label, policy, mut c, a, h, want, frag)) in cases.into_iter().enumerate() {
        c.run_id = format!("case-{n}");
        let d = fx.engine.decide(&policy, &c, a.as_ref(), &h);
        let got = match d {
            Decision::Allow { .. } => 'A',
            Decision::Ask { .. } => 'K',
            Decision::Deny { .. } => 'D',
        };
        assert_eq!(got, want, "case '{label}': got {d:?}");
        assert!(
            reason(&d).contains(frag),
            "case '{label}': reason '{}' should name rule '{frag}'",
            reason(&d)
        );
    }
}

// ---- tiers

#[test]
fn a_jail_auto_allows_only_when_it_enforces_deny_paths() {
    let fx = fixture();
    let login = host("web-1", &["staging"], JailKind::Landlock);
    let agent = host_trusting_ca("web-1", &["staging"], JailKind::Landlock);
    let bwrap = host("web-1", &["staging"], JailKind::Bubblewrap);
    let bwrap_agent = host_trusting_ca("web-1", &["staging"], JailKind::Bubblewrap);
    let c = cmd_call(ToolName::RunReadonly, "web-1", "rm x");
    let wr = analysis("rm", &["x"], &[RiskTag::Destructive]);

    // As the login user, Landlock can't mask deny_paths and bubblewrap only masks literal
    // ones: the analyzer decides.
    for h in [&login, &bwrap] {
        let d = fx.engine.decide(&operate(), &c, Some(&wr), h);
        assert!(matches!(d, Decision::Ask { .. }), "{h:?}: {d:?}");
        assert!(
            reason(&d).contains("cannot enforce deny_paths"),
            "{}",
            reason(&d)
        );
    }
    // The low-privilege agent principal does provide it, with either jail.
    for h in [&agent, &bwrap_agent] {
        let d = fx.engine.decide(&operate(), &c, Some(&wr), h);
        assert!(is_allow(&d), "{h:?}: {d:?}");
        assert!(reason(&d).starts_with("jail:"), "{}", reason(&d));
    }
    // A provably read-only command still runs under the login-user Landlock jail.
    let ro = analysis("cat", &["/etc/hosts"], &[RiskTag::Read]);
    let d = fx.engine.decide(&operate(), &c, Some(&ro), &login);
    assert!(is_allow(&d), "{d:?}");
    assert!(reason(&d).starts_with("analyzer:"), "{}", reason(&d));
}

#[test]
fn tier_two_person_for_critical_tags() {
    let fx = fixture();
    let mut p = operate();
    p.approval.two_person = vec!["prod-db".into()];
    p.scope.tags.push("prod-db".into());
    let h = host("db-1", &["prod-db", "prod"], JailKind::None);
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "db-1", "ls"), None, &h);
    assert_eq!(tier(&d), Tier::TwoPerson);
}

#[test]
fn tier_two_person_beats_in_client() {
    let fx = fixture();
    let mut p = operate();
    p.approval.two_person = vec!["prod".into()];
    p.approval.in_client = vec!["prod".into()];
    let h = host("web-1", &["prod"], JailKind::None);
    let a = analysis("systemctl", &["reload", "nginx"], &[RiskTag::ServiceChange]);
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), Some(&a), &h);
    assert_eq!(tier(&d), Tier::TwoPerson);
}

#[test]
fn tier_in_client_for_low_risk_on_listed_tag() {
    let fx = fixture();
    let mut p = operate();
    p.approval.in_client = vec!["staging".into()];
    let h = host("web-1", &["staging"], JailKind::None);
    let a = analysis("systemctl", &["reload", "nginx"], &[RiskTag::ServiceChange]);
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), Some(&a), &h);
    assert_eq!(tier(&d), Tier::InClient);
}

#[test]
fn tier_in_client_not_for_risky_tags() {
    for risky in [
        RiskTag::Destructive,
        RiskTag::Privilege,
        RiskTag::Network,
        RiskTag::ExfilRisk,
        RiskTag::Unknown,
    ] {
        let fx = fixture();
        let mut p = operate();
        p.approval.in_client = vec!["staging".into()];
        let h = host("web-1", &["staging"], JailKind::None);
        let a = analysis("x", &[], &[RiskTag::Write, risky]);
        let d = fx
            .engine
            .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), Some(&a), &h);
        assert_eq!(tier(&d), Tier::OutOfBand, "{risky:?}");
    }
}

#[test]
fn tier_in_client_not_for_unanalyzed_run_or_unknown_reason() {
    let fx = fixture();
    let mut p = operate();
    p.approval.in_client = vec!["staging".into()];
    let h = host("web-1", &["staging"], JailKind::None);
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), None, &h);
    assert_eq!(tier(&d), Tier::OutOfBand);
    let d = fx.engine.decide(
        &p,
        &cmd_call(ToolName::Run, "web-1", "x"),
        Some(&unknown_analysis()),
        &h,
    );
    assert_eq!(tier(&d), Tier::OutOfBand);
}

#[test]
fn tier_in_client_for_edit_file_and_service_on_listed_tag() {
    let fx = fixture();
    let mut p = operate();
    p.approval.in_client = vec!["staging".into()];
    let h = host("web-1", &["staging"], JailKind::None);
    let d = fx.engine.decide(
        &p,
        &path_call(ToolName::EditFile, "web-1", "/etc/nginx/a"),
        None,
        &h,
    );
    assert_eq!(tier(&d), Tier::InClient);
    let d = fx
        .engine
        .decide(&p, &call(ToolName::Service, Some("web-1")), None, &h);
    assert_eq!(tier(&d), Tier::InClient);
}

#[test]
fn tier_in_client_never_for_untagged_prod() {
    let fx = fixture();
    let mut p = operate();
    p.approval.in_client = vec!["staging".into()];
    let h = host("web-9", &["prod"], JailKind::None);
    let d = fx.engine.decide(
        &p,
        &path_call(ToolName::EditFile, "web-9", "/etc/nginx/a"),
        None,
        &h,
    );
    assert_eq!(tier(&d), Tier::OutOfBand);
}

#[test]
fn untrusted_tags_do_not_allow_in_client_approval() {
    let fx = fixture();
    let mut p = operate();
    // The host is in scope by name: only the in-client tag is at issue.
    p.scope = Scope {
        tags: Vec::new(),
        hosts: vec!["web-*".into()],
    };
    p.approval.in_client = vec!["staging".into()];

    let untrusted = untrusted_host("web-1", &["staging"], "aws-prod");
    let d = fx.engine.decide(
        &p,
        &path_call(ToolName::EditFile, "web-1", "/etc/nginx/a"),
        None,
        &untrusted,
    );
    assert_eq!(tier(&d), Tier::OutOfBand);
    let why = reason(&d);
    assert!(why.contains("ai.trusted_tag_sources"), "{why}");
    assert!(why.contains("aws-prod"), "{why}");

    let trusted = host("web-1", &["staging"], JailKind::None);
    let d = fx.engine.decide(
        &p,
        &path_call(ToolName::EditFile, "web-1", "/etc/nginx/a"),
        None,
        &trusted,
    );
    assert_eq!(tier(&d), Tier::InClient);
}

#[test]
fn two_person_applies_from_untrusted_tags() {
    let fx = fixture();
    let mut p = operate();
    p.scope = Scope {
        tags: Vec::new(),
        hosts: vec!["db-*".into()],
    };
    p.approval.two_person = vec!["prod-db".into()];
    // A tag from a source nobody listed must still tighten the decision.
    let h = untrusted_host("db-1", &["prod-db"], "aws-prod");
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "db-1", "ls"), None, &h);
    assert_eq!(tier(&d), Tier::TwoPerson);
}

#[test]
fn two_person_hosts_glob_needs_no_tag() {
    let fx = fixture();
    let mut p = operate();
    p.scope = Scope {
        tags: Vec::new(),
        hosts: vec!["db-*".into(), "web-*".into()],
    };
    p.approval.two_person_hosts = vec!["db-*".into()];
    let h = host("db-1", &[], JailKind::None);
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "db-1", "ls"), None, &h);
    assert_eq!(tier(&d), Tier::TwoPerson);

    // A host outside the glob keeps the default tier.
    let h = host("web-1", &[], JailKind::None);
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "web-1", "ls"), None, &h);
    assert_eq!(tier(&d), Tier::OutOfBand);
}

#[test]
fn tier_follows_configured_default() {
    let fx = fixture();
    let mut p = operate();
    p.approval.default = Tier::TwoPerson;
    let h = host("web-1", &["staging"], JailKind::None);
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), None, &h);
    assert_eq!(tier(&d), Tier::TwoPerson);
}

// ---- grants

fn grant(tool: ToolName, hosts: &str, target: Option<&str>, run: Option<&str>) -> Grant {
    Grant {
        grant_id: "g1".into(),
        run_id: run.map(String::from),
        tool,
        hosts: hosts.into(),
        target: target.map(String::from),
        expires_at: (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
    }
}

#[test]
fn grant_turns_ask_into_allow() {
    let fx = fixture();
    fx.store
        .add_grant(&grant(
            ToolName::EditFile,
            "web-*",
            Some("/etc/nginx/**"),
            None,
        ))
        .unwrap();
    let h = host("web-1", &["staging"], JailKind::None);
    let d = fx.engine.decide(
        &operate(),
        &path_call(ToolName::EditFile, "web-1", "/etc/nginx/a.conf"),
        None,
        &h,
    );
    assert!(is_allow(&d), "{d:?}");
    assert!(reason(&d).contains("grant: g1"));
}

#[test]
fn grant_for_another_tool_host_or_target_does_not_apply() {
    let fx = fixture();
    fx.store
        .add_grant(&grant(
            ToolName::EditFile,
            "web-*",
            Some("/etc/nginx/**"),
            None,
        ))
        .unwrap();
    let h = host("web-1", &["staging"], JailKind::None);
    let p = operate();
    let d = fx.engine.decide(
        &p,
        &path_call(ToolName::EditFile, "web-1", "/etc/other/a"),
        None,
        &h,
    );
    assert!(matches!(d, Decision::Ask { .. }));
    let d = fx
        .engine
        .decide(&p, &call(ToolName::Service, Some("web-1")), None, &h);
    assert!(matches!(d, Decision::Ask { .. }));
    let h2 = host("app-1", &["staging"], JailKind::None);
    let d = fx.engine.decide(
        &p,
        &path_call(ToolName::EditFile, "app-1", "/etc/nginx/a"),
        None,
        &h2,
    );
    assert!(matches!(d, Decision::Ask { .. }));
}

#[test]
fn grant_for_service_unit() {
    let fx = fixture();
    fx.store
        .add_grant(&grant(ToolName::Service, "*", Some("nginx*"), Some("run1")))
        .unwrap();
    let h = host("web-1", &["staging"], JailKind::None);
    let mut c = call(ToolName::Service, Some("web-1"));
    c.args = serde_json::json!({"unit": "nginx.service", "action": "reload"});
    assert!(is_allow(&fx.engine.decide(&operate(), &c, None, &h)));
    c.args = serde_json::json!({"unit": "postgres.service"});
    assert!(matches!(
        fx.engine.decide(&operate(), &c, None, &h),
        Decision::Ask { .. }
    ));
}

#[test]
fn grant_of_another_run_is_ignored() {
    let fx = fixture();
    fx.store
        .add_grant(&grant(ToolName::Service, "*", None, Some("other-run")))
        .unwrap();
    let h = host("web-1", &["staging"], JailKind::None);
    let d = fx.engine.decide(
        &operate(),
        &call(ToolName::Service, Some("web-1")),
        None,
        &h,
    );
    assert!(matches!(d, Decision::Ask { .. }));
}

#[test]
fn revoked_and_expired_grants_do_not_apply() {
    let fx = fixture();
    fx.store
        .add_grant(&grant(ToolName::Service, "*", None, None))
        .unwrap();
    fx.store.revoke_grant("g1").unwrap();
    let mut old = grant(ToolName::Service, "*", None, None);
    old.grant_id = "g2".into();
    old.expires_at = (Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    fx.store.add_grant(&old).unwrap();
    let h = host("web-1", &["staging"], JailKind::None);
    let d = fx.engine.decide(
        &operate(),
        &call(ToolName::Service, Some("web-1")),
        None,
        &h,
    );
    assert!(matches!(d, Decision::Ask { .. }));
}

#[test]
fn grant_never_overrides_deny() {
    let fx = fixture();
    fx.store
        .add_grant(&grant(ToolName::Run, "*", None, None))
        .unwrap();
    let h = host("web-1", &["staging"], JailKind::None);
    let a = analysis("rm", &["-rf", "/"], &[RiskTag::Destructive]);
    let d = fx.engine.decide(
        &operate(),
        &cmd_call(ToolName::Run, "web-1", "rm -rf /"),
        Some(&a),
        &h,
    );
    assert!(is_deny(&d), "{d:?}");
    // and not over the observe profile either
    let mut p = operate();
    p.profile = Profile::Observe;
    let d = fx
        .engine
        .decide(&p, &cmd_call(ToolName::Run, "web-1", "ls"), None, &h);
    assert!(is_deny(&d));
}

// ---- budgets and circuit breaker

#[test]
fn calls_per_min_budget_denies_over_limit() {
    let fx = fixture();
    let mut p = operate();
    p.budgets.calls_per_min = 3;
    let h = host("web-1", &["staging"], JailKind::None);
    let c = call(ToolName::Observe, Some("web-1"));
    for _ in 0..3 {
        assert!(is_allow(&fx.engine.decide(&p, &c, None, &h)));
    }
    let d = fx.engine.decide(&p, &c, None, &h);
    assert!(is_deny(&d));
    assert!(reason(&d).contains("budget.calls_per_min"));
}

#[test]
fn calls_per_min_zero_means_unlimited() {
    let fx = fixture();
    let mut p = operate();
    p.budgets.calls_per_min = 0;
    let h = host("web-1", &["staging"], JailKind::None);
    for _ in 0..50 {
        assert!(is_allow(&fx.engine.decide(
            &p,
            &call(ToolName::Observe, Some("web-1")),
            None,
            &h
        )));
    }
}

#[test]
fn calls_budget_is_per_run() {
    let fx = fixture();
    let mut p = operate();
    p.budgets.calls_per_min = 1;
    let h = host("web-1", &["staging"], JailKind::None);
    let mut c = call(ToolName::Observe, Some("web-1"));
    assert!(is_allow(&fx.engine.decide(&p, &c, None, &h)));
    c.run_id = "run2".into();
    assert!(is_allow(&fx.engine.decide(&p, &c, None, &h)));
}

#[test]
fn changes_per_run_budget_denies_writes_only() {
    let fx = fixture();
    let mut p = operate();
    p.budgets.changes_per_run = 2;
    let h = host("web-1", &["staging"], JailKind::None);
    let w = cmd_call(ToolName::Run, "web-1", "touch x");
    assert!(matches!(
        fx.engine.decide(&p, &w, None, &h),
        Decision::Ask { .. }
    ));
    fx.store.note_change("run1", "a").unwrap();
    fx.store.note_change("run1", "b").unwrap();
    let d = fx.engine.decide(&p, &w, None, &h);
    assert!(is_deny(&d));
    assert!(reason(&d).contains("budget.changes_per_run"));
    // reads still fine
    assert!(is_allow(&fx.engine.decide(
        &p,
        &call(ToolName::Observe, Some("web-1")),
        None,
        &h
    )));
}

#[test]
fn run_ttl_budget_denies_old_runs() {
    use crate::services::ai::store::RunRecord;
    let fx = fixture();
    fx.store
        .start_run(&RunRecord {
            id: "run1".into(),
            client: "c".into(),
            on_behalf_of: "alice".into(),
            policy: "default".into(),
            started: (Utc::now() - chrono::Duration::hours(3)).to_rfc3339(),
            ended: None,
            reason: None,
        })
        .unwrap();
    let h = host("web-1", &["staging"], JailKind::None);
    let d = fx.engine.decide(
        &operate(),
        &call(ToolName::Observe, Some("web-1")),
        None,
        &h,
    );
    assert!(is_deny(&d));
    assert!(reason(&d).contains("budget.run_ttl"));
}

#[test]
fn denials_are_counted_and_trip_the_breaker() {
    let fx = fixture();
    let mut p = operate();
    p.budgets.max_denials_per_min = 3;
    let h = host("web-1", &["staging"], JailKind::None);
    let bad = path_call(ToolName::ReadFile, "web-1", "/etc/shadow");
    for _ in 0..2 {
        assert!(is_deny(&fx.engine.decide(&p, &bad, None, &h)));
    }
    assert!(circuit_breaker(&fx.store, "run1", &p.budgets).is_none());
    assert!(is_deny(&fx.engine.decide(&p, &bad, None, &h)));
    let why = circuit_breaker(&fx.store, "run1", &p.budgets).unwrap();
    assert!(why.contains("denied"), "{why}");
    assert!(circuit_breaker(&fx.store, "other", &p.budgets).is_none());
}

#[test]
fn breaker_off_when_limit_is_zero() {
    let fx = fixture();
    let mut p = operate();
    p.budgets.max_denials_per_min = 0;
    let h = host("web-1", &["staging"], JailKind::None);
    let bad = path_call(ToolName::ReadFile, "web-1", "/etc/shadow");
    for _ in 0..10 {
        fx.engine.decide(&p, &bad, None, &h);
    }
    assert!(circuit_breaker(&fx.store, "run1", &p.budgets).is_none());
}

#[test]
fn probing_out_of_scope_hosts_trips_the_breaker() {
    let fx = fixture();
    let mut p = operate();
    p.budgets.max_denials_per_min = 0;
    let other = host("db-9", &["lab"], JailKind::None);
    for i in 0..PROBE_LIMIT {
        assert!(
            circuit_breaker(&fx.store, "run1", &p.budgets).is_none(),
            "{i}"
        );
        fx.engine
            .decide(&p, &call(ToolName::Observe, Some("db-9")), None, &other);
    }
    let why = circuit_breaker(&fx.store, "run1", &p.budgets).unwrap();
    assert!(why.contains("outside the policy scope"), "{why}");
}

// ---- resolve / scope

#[test]
fn missing_default_is_observe_over_every_host() {
    let cfg = AppConfig::default();
    let p = resolve(&cfg, "default").unwrap();
    assert_eq!(p.profile, Profile::Observe);
    assert!(in_scope(&p.scope, "anything", &[], TagTrust::Trusted));
    assert!(!p.read_paths.is_empty());
    assert!(!p.deny_paths.is_empty());
    assert!(!p.deny.is_empty());
    assert_eq!(p.budgets.changes_per_run, 0);
}

#[test]
fn unknown_named_policy_is_an_error() {
    let mut cfg = AppConfig::default();
    cfg.ai.policies.insert("ops".into(), Policy::default());
    let e = resolve(&cfg, "nope").unwrap_err().to_string();
    assert!(e.contains("nope") && e.contains("ops"), "{e}");
}

#[test]
fn explicit_policy_with_empty_scope_means_no_host() {
    let mut cfg = AppConfig::default();
    cfg.ai.policies.insert("default".into(), Policy::default());
    let p = resolve(&cfg, "default").unwrap();
    assert!(!in_scope(
        &p.scope,
        "web-1",
        &["staging".into()],
        TagTrust::Trusted
    ));
}

#[test]
fn resolve_merges_user_policy_over_builtin() {
    let mut cfg = AppConfig::default();
    let user: Policy = serde_json::from_str(
        r#"{"profile":"operate","scope":{"tags":["staging"]},
            "deny_paths":["/srv/secret/**"],"deny":["kill -9 *"],
            "read_paths":["/opt/**"],"budgets":{"calls_per_min":5}}"#,
    )
    .unwrap();
    cfg.ai.policies.insert("ops".into(), user);
    let p = resolve(&cfg, "ops").unwrap();
    assert_eq!(p.profile, Profile::Operate);
    assert_eq!(p.read_paths, vec!["/opt/**".to_string()]);
    assert!(p.deny_paths.contains(&"/srv/secret/**".to_string()));
    assert!(
        p.deny_paths.contains(&"/etc/shadow".to_string()),
        "built-ins stay"
    );
    assert!(p.deny.contains(&"kill -9 *".to_string()));
    assert!(p.deny.contains(&"mkfs *".to_string()));
    assert_eq!(p.budgets.calls_per_min, 5);
}

#[test]
fn resolve_uses_profile_budgets_when_none_set() {
    let mut cfg = AppConfig::default();
    let user: Policy =
        serde_json::from_str(r#"{"profile":"oncall","scope":{"tags":["x"]}}"#).unwrap();
    cfg.ai.policies.insert("oc".into(), user);
    let p = resolve(&cfg, "oc").unwrap();
    assert_eq!(p.budgets.calls_per_min, 60);
    assert_eq!(p.budgets.changes_per_run, 20);
}

#[test]
fn resolve_keeps_user_empty_write_paths_and_approval() {
    let mut cfg = AppConfig::default();
    let user: Policy = serde_json::from_str(
        r#"{"profile":"operate","approval":{"two_person":["prod-db"],"two_person_hosts":["db-*"]},"write_paths":["/etc/nginx/**"]}"#,
    )
    .unwrap();
    cfg.ai.policies.insert("ops".into(), user);
    let p = resolve(&cfg, "ops").unwrap();
    assert_eq!(p.approval.two_person, vec!["prod-db".to_string()]);
    assert_eq!(p.approval.two_person_hosts, vec!["db-*".to_string()]);
    assert_eq!(p.write_paths, vec!["/etc/nginx/**".to_string()]);
}

#[test]
fn host_in_scope_by_tag_name_and_wildcard() {
    let mut conn = Connection::new("web-1".into(), "h".into(), "u".into(), 22);
    conn.tags = vec!["staging".into()];
    let mut p = Policy::default();
    assert!(!host_in_scope(&p, &conn, TagTrust::Trusted));
    p.scope.tags = vec!["staging".into()];
    assert!(host_in_scope(&p, &conn, TagTrust::Trusted));
    p.scope.tags = vec!["prod".into()];
    assert!(!host_in_scope(&p, &conn, TagTrust::Trusted));
    p.scope.hosts = vec!["web-*".into()];
    assert!(host_in_scope(&p, &conn, TagTrust::Trusted));
    p.scope = Scope {
        tags: vec![ALL_HOSTS.into()],
        hosts: vec![],
    };
    assert!(host_in_scope(&p, &conn, TagTrust::Trusted));
}

#[test]
fn source_tags_count_for_scope_only_when_trusted() {
    let fx = fixture();
    let mut p = builtin_profile(Profile::Observe);
    p.scope.tags = vec!["prod".into()];

    let untrusted = untrusted_host("db-1", &["prod"], "aws-prod");
    let d = fx
        .engine
        .decide(&p, &call(ToolName::Observe, Some("db-1")), None, &untrusted);
    assert!(matches!(&d, Decision::Deny { .. }), "{d:?}");
    let why = reason(&d);
    assert!(why.contains("not in this policy's scope"), "{why}");
    assert!(why.contains("ai.trusted_tag_sources"), "{why}");
    assert!(why.contains("aws-prod"), "{why}");

    let trusted = host("db-1", &["prod"], JailKind::None);
    let d = fx
        .engine
        .decide(&p, &call(ToolName::Observe, Some("db-1")), None, &trusted);
    assert!(matches!(&d, Decision::Allow { .. }), "{d:?}");
}

#[test]
fn connection_facts_follow_the_connection_source() {
    let mut conn = Connection::new("web-1".into(), "h".into(), "u".into(), 22);
    conn.tags = vec!["prod".into()];

    let local = connection_facts(&conn, None, &[], Enforcement::default());
    assert!(local.tags_trusted);
    assert_eq!(local.tag_source, None);

    conn.source_id = Some(uuid::Uuid::new_v4());
    let untrusted = connection_facts(&conn, Some("aws-prod"), &[], Enforcement::default());
    assert!(!untrusted.tags_trusted);
    assert_eq!(untrusted.tag_source.as_deref(), Some("aws-prod"));
    assert_eq!(untrusted.tags, vec!["prod".to_string()]);

    let trusted = connection_facts(
        &conn,
        Some("aws-prod"),
        &["aws-prod".to_string()],
        Enforcement::default(),
    );
    assert!(trusted.tags_trusted);
    // A listed source whose name no longer resolves is not trusted.
    assert_eq!(
        tags_trusted(&["aws-prod".to_string()], &conn, None),
        TagTrust::Untrusted
    );
}

#[test]
fn approval_rules_two_person_hosts_default_and_round_trip() {
    let empty: ApprovalRules = serde_json::from_str("{}").unwrap();
    assert!(empty.two_person_hosts.is_empty());

    let rules = ApprovalRules {
        two_person_hosts: vec!["db-*".into()],
        ..ApprovalRules::default()
    };
    let json = serde_json::to_string(&rules).unwrap();
    assert_eq!(serde_json::from_str::<ApprovalRules>(&json).unwrap(), rules);
}
