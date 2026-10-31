use super::decisions::reason;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::policy::candidates::path_candidates;
use crate::services::ai::policy::deny_paths::{deny_path_hit, Candidate, Expansion, PathHit};
use crate::services::ai::policy::glob::{glob_match, normalize_path, GlobMode};
use crate::services::ai::policy::{builtin_profile, parse_duration, DefaultPolicyEngine};
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    Analysis, Analyzer, Decision, Enforcement, Grant, HostFacts, Policy, PolicyEngine, Profile,
    ToolCall, ToolName,
};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;

// ---- helpers

#[test]
fn glob_basics() {
    assert!(glob_match("web-*", "web-12", GlobMode::Text));
    assert!(!glob_match("web-*", "db-1", GlobMode::Text));
    assert!(glob_match("mkfs *", "mkfs /dev/sda", GlobMode::Text));
    assert!(glob_match("a?c", "abc", GlobMode::Text));
    assert!(glob_match("*", "", GlobMode::Text));
    assert!(!glob_match("a", "ab", GlobMode::Text));
}

#[test]
fn glob_path_mode() {
    assert!(glob_match(
        "/var/log/**",
        "/var/log/a/b.log",
        GlobMode::Path
    ));
    assert!(!glob_match(
        "/var/log/*",
        "/var/log/a/b.log",
        GlobMode::Path
    ));
    assert!(glob_match("/var/log/*", "/var/log/a.log", GlobMode::Path));
    assert!(glob_match("**/.env", "/.env", GlobMode::Path));
    assert!(glob_match("**/.env", "/srv/app/.env", GlobMode::Path));
    assert!(!glob_match("**/.env", "/srv/app/.envrc", GlobMode::Path));
    assert!(glob_match("/etc/**/x", "/etc/x", GlobMode::Path));
    assert!(glob_match("**/*.key", "/a/b/site.key", GlobMode::Path));
    assert!(glob_match(
        "**/id_rsa*",
        "/home/u/id_rsa.pub",
        GlobMode::Path
    ));
}

#[test]
fn normalize_paths() {
    assert_eq!(normalize_path("/a/b/../c"), "/a/c");
    assert_eq!(normalize_path("/a//b/./c"), "/a/b/c");
    assert_eq!(normalize_path("/../.."), "/");
    assert_eq!(normalize_path("rel/../x"), "rel/../x");
}

#[test]
fn durations() {
    assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
    assert_eq!(parse_duration("1h30m"), Some(Duration::from_secs(5400)));
    assert_eq!(parse_duration("45s"), Some(Duration::from_secs(45)));
    assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
    assert_eq!(parse_duration("90"), Some(Duration::from_secs(90)));
    assert_eq!(parse_duration(""), None);
    assert_eq!(parse_duration("2x"), None);
    assert_eq!(parse_duration("h"), None);
    assert_eq!(parse_duration("5m3"), None);
}

// ---- deny paths, globs and cd tracking

fn operated() -> Policy {
    let mut p = builtin_profile(Profile::Operate);
    p.scope.tags = vec!["staging".into()];
    p
}

fn plain_host() -> HostFacts {
    HostFacts {
        name: "web-1".into(),
        tags: vec!["staging".into()],
        tags_trusted: true,
        enforcement: Enforcement::default(),
        ..HostFacts::default()
    }
}

fn analyzed(command: &str) -> Analysis {
    ShellAnalyzer::new().analyze(command)
}

fn readonly_call(command: &str) -> ToolCall {
    ToolCall {
        run_id: "run1".into(),
        call_id: "c1".into(),
        tool: ToolName::RunReadonly,
        host: Some("web-1".into()),
        command: Some(command.into()),
        path: None,
        args: serde_json::Value::Null,
    }
}

fn file_call(path: &str) -> ToolCall {
    ToolCall {
        tool: ToolName::ReadFile,
        path: Some(path.into()),
        ..readonly_call("")
    }
}

/// `run_readonly` on a plain host, with the command really analyzed.
fn decide(command: &str) -> Decision {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
    let engine = DefaultPolicyEngine::new(store);
    engine.decide(
        &operated(),
        &readonly_call(command),
        Some(&analyzed(command)),
        &plain_host(),
    )
}

#[test]
fn deny_paths_covers_operands_redirections_and_globs() {
    for cmd in [
        // input redirections were not recorded at all before
        "cat < /etc/shadow",
        "cat 0</etc/shadow",
        "cat <>/etc/shadow",
        "{ cat; } < /etc/shadow",
        // programs that copy a deny path to stdout
        "base64 /etc/shadow",
        "xxd /etc/shadow",
        "od -c /etc/shadow",
        "hexdump -C /etc/shadow",
        "strings /etc/shadow",
        // unexpanded globs stand for the path they name
        "cat /etc/sha*",
        "cat /etc/shado?",
        "cat /etc/[s]hadow",
        "cat /etc/*",
        // a literal `cd` changes where relative arguments resolve
        "cd /etc && cat shadow",
        "cd /root && cat .ssh/id_rsa",
        // a relative path with no `cd` still matches the `**/` deny entries
        "cat .ssh/id_rsa",
        // home-relative arguments
        "cat ~/.ssh/id_rsa",
        "cat $HOME/.ssh/id_rsa",
        "cat ${HOME}/.ssh/id_rsa",
        "cat ~root/.ssh/id_ed25519",
        // values attached to options
        "diff --to-file=/etc/shadow /etc/hosts",
        "diff -f/etc/shadow /etc/hosts",
        // new default deny entries
        "cat /etc/krb5.keytab",
        "cat /etc/ssl/private/site.key",
        "cat /etc/ssh/ssh_host_rsa_key",
        "cat /root/.bashrc",
        "cat /proc/1/environ",
        "cat ~/.netrc",
        "cat ~/.pgpass",
        "cat ~/.kube/config",
        "cat ~/.docker/config.json",
        "cat /etc/rancher/k3s/config.yaml",
        "cat /etc/kubernetes/admin.conf",
        "cat /etc/security/opasswd",
        "cat /etc/sudoers",
        "cat /etc/gshadow-",
    ] {
        let d = decide(cmd);
        assert!(matches!(d, Decision::Deny { .. }), "{cmd:?}: {d:?}");
        assert!(
            reason(&d).starts_with("deny_paths:"),
            "{cmd:?}: {}",
            reason(&d)
        );
    }
}

#[test]
fn unprovable_paths_ask_and_grants_do_not_cover_them() {
    // A glob that only *might* be a deny path is not provable: ask, never allow.
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
    store
        .add_grant(&Grant {
            grant_id: "g1".into(),
            run_id: None,
            tool: ToolName::RunReadonly,
            hosts: "*".into(),
            target: None,
            expires_at: (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
        })
        .unwrap();
    let engine = DefaultPolicyEngine::new(store);
    for (cmd, frag) in [
        ("cat /etc/ssh/*", "could expand"),
        ("cat $DIR/notes", "run time"),
        ("cat ${DIR:-/etc}/shadow", "run time"),
    ] {
        let d = engine.decide(
            &operated(),
            &readonly_call(cmd),
            Some(&analyzed(cmd)),
            &plain_host(),
        );
        assert!(matches!(d, Decision::Ask { .. }), "{cmd:?}: {d:?}");
        assert!(reason(&d).contains(frag), "{cmd:?}: {}", reason(&d));
    }
    // A `cd` we can't follow is never allowed: the `$D` target is a run-time path and
    // the relative `shadow` after it is unprovable.
    let cmd = "cd $D && cat shadow";
    let d = engine.decide(
        &operated(),
        &readonly_call(cmd),
        Some(&analyzed(cmd)),
        &plain_host(),
    );
    assert!(matches!(d, Decision::Ask { .. }), "{d:?}");
    assert!(
        reason(&d).starts_with("deny_paths:") || reason(&d).starts_with("analyzer:"),
        "{}",
        reason(&d)
    );
}

#[test]
fn read_only_commands_without_secrets_still_pass() {
    for cmd in [
        "cat /etc/hosts",
        "cat /etc/os-release",
        "ls -la /var/log",
        "cat /etc/ssh/ssh_host_rsa_key.pub",
        "cat /etc/ssh/sshd_config",
        "journalctl -u nginx --since '1 hour ago'",
        "find /var/log -name '*.log' -mtime +7",
        "cd /tmp && ls",
        "diff /etc/hosts /etc/hostname",
        "grep -r error /var/log/nginx",
        "awk -F: '{print $1}' /etc/passwd",
    ] {
        let d = decide(cmd);
        assert!(
            matches!(d, Decision::Allow { .. }),
            "{cmd:?}: {d:?} ({})",
            reason(&d)
        );
    }
}

#[test]
fn candidates_cover_option_values_and_home_paths() {
    let texts = |a: &Analysis| -> Vec<String> {
        path_candidates(Some(a))
            .0
            .into_iter()
            .map(|c| c.text)
            .collect()
    };
    let a = analyzed("diff --to-file=/etc/shadow -f/tmp/x a");
    let (cands, unsure) = path_candidates(Some(&a));
    let got: Vec<&str> = cands.iter().map(|c| c.text.as_str()).collect();
    assert!(got.contains(&"/etc/shadow"), "{got:?}");
    assert!(got.contains(&"/tmp/x"), "{got:?}");
    assert!(got.contains(&"a"), "{got:?}");
    assert!(unsure.is_none());

    let a = analyzed("cat ${HOME}/.ssh/id_rsa");
    assert_eq!(texts(&a), vec![".ssh/id_rsa".to_string()]);

    let a = analyzed("cat $DIR/x");
    let (_, unsure) = path_candidates(Some(&a));
    assert_eq!(unsure.as_deref(), Some("$DIR/x"));

    // `$(...)` is not a variable reference: the analyzer walks what is inside.
    let a = analyzed("cat $(pwd)");
    let (_, unsure) = path_candidates(Some(&a));
    assert!(unsure.is_none(), "{unsure:?}");
}

#[test]
fn default_deny_paths_cover_secrets_and_keep_public_host_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
    let engine = DefaultPolicyEngine::new(store);
    let decide_path = |p: &str| engine.decide(&operated(), &file_call(p), None, &plain_host());
    for p in [
        "/etc/ssh/ssh_host_rsa_key",
        "/etc/ssh/ssh_host_ed25519_key",
        "/etc/krb5.keytab",
        "/var/lib/secrets/app.keytab",
        "/etc/ssl/private/site.key",
        "/etc/wireguard/wg0.conf",
        "/etc/gshadow-",
        "/etc/security/opasswd",
        "/etc/sudoers",
        "/root/.bashrc",
        "/home/u/.kube/config",
        "/etc/kubernetes/manifests/admin.conf",
        "/etc/rancher/k3s/config.yaml",
        "/home/u/.netrc",
        "/home/u/.pgpass",
        "/home/u/.my.cnf",
        "/home/u/.docker/config.json",
        "/srv/app/.git-credentials",
        "/proc/1/environ",
        "/proc/2/mem",
    ] {
        assert!(matches!(decide_path(p), Decision::Deny { .. }), "{p}");
    }
    for p in [
        "/etc/ssh/ssh_host_rsa_key.pub",
        "/etc/ssh/ssh_host_ed25519_key.pub",
        "/etc/ssh/sshd_config",
        "/etc/hostname",
    ] {
        assert!(
            matches!(decide_path(p), Decision::Allow { .. }),
            "{p}: {:?}",
            decide_path(p)
        );
    }
}

#[test]
fn glob_matching_stays_polynomial() {
    // Backtracking over this many stars was exponential; the DP is O(pattern × text).
    let text = "a".repeat(20_000);
    let pattern = format!("{}b", "*a".repeat(40));
    let start = std::time::Instant::now();
    assert!(!glob_match(&pattern, &text, GlobMode::Text));
    assert!(glob_match(&pattern, &format!("{text}b"), GlobMode::Text));
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn glob_classes() {
    assert!(glob_match("/etc/[s]hadow", "/etc/shadow", GlobMode::Path));
    assert!(glob_match("/etc/[a-z]hadow", "/etc/shadow", GlobMode::Path));
    assert!(glob_match("/etc/[!x]hadow", "/etc/shadow", GlobMode::Path));
    assert!(!glob_match("/etc/[!s]hadow", "/etc/shadow", GlobMode::Path));
    // An unclosed `[` is a literal bracket, as in the shell.
    assert!(glob_match("/etc/[x", "/etc/[x", GlobMode::Path));
    assert!(!glob_match("/etc/[x", "/etc/x", GlobMode::Path));
    // Globs stay globs for a literal deny path.
    let glob = |t: &str| Candidate {
        text: t.into(),
        expansion: Expansion::Glob,
    };
    assert!(deny_path_hit(&["/etc/shadow".into()], &glob("/etc/[s]hadow")).is_some());
    assert!(matches!(
        deny_path_hit(&["/etc/shadow".into()], &glob("/etc/shado?")),
        Some(PathHit::Deny(_))
    ));
    // Two globs only tell us a path is possible.
    assert!(matches!(
        deny_path_hit(&["**/id_rsa*".into()], &glob(".ssh/id_rs*")),
        Some(PathHit::Unsure(_))
    ));
    assert!(deny_path_hit(&["**/*.key".into()], &glob("/var/log/*.log")).is_none());
    // A quoted pattern is not expanded by the shell: it names no path.
    let literal = Candidate {
        text: "*.log".into(),
        expansion: Expansion::Literal,
    };
    assert!(deny_path_hit(&["**/id_rsa*".into()], &literal).is_none());
}

// ---- glob semantics

/// The recursive matcher the DP one replaced, kept as the reference for its semantics.
/// `[` classes are the one deliberate extension, so they stay out of the alphabet.
fn old_rec(p: &[char], t: &[char], path_mode: bool) -> bool {
    match p.first() {
        None => t.is_empty(),
        Some('*') => {
            let double = path_mode && p.get(1) == Some(&'*');
            if double {
                if p.get(2) == Some(&'/') && old_rec(&p[3..], t, path_mode) {
                    return true;
                }
                (0..=t.len()).any(|i| old_rec(&p[2..], &t[i..], path_mode))
            } else {
                let mut i = 0;
                loop {
                    if old_rec(&p[1..], &t[i..], path_mode) {
                        return true;
                    }
                    if i >= t.len() || (path_mode && t[i] == '/') {
                        return false;
                    }
                    i += 1;
                }
            }
        }
        Some('?') => match t.first() {
            Some(c) if !(path_mode && *c == '/') => old_rec(&p[1..], &t[1..], path_mode),
            _ => false,
        },
        Some(c) => t.first() == Some(c) && old_rec(&p[1..], &t[1..], path_mode),
    }
}

fn old_glob(pattern: &str, text: &str, path_mode: bool) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    old_rec(&p, &t, path_mode)
}

#[test]
fn new_matcher_agrees_with_the_old_one() {
    let alphabet = ['a', 'b', '/', '*', '?'];
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    for path_mode in [false, true] {
        let mode = if path_mode {
            GlobMode::Path
        } else {
            GlobMode::Text
        };
        for _ in 0..20_000 {
            let plen = next(7) + 1;
            let tlen = next(9);
            let pat: String = (0..plen).map(|_| alphabet[next(alphabet.len())]).collect();
            let text: String = (0..tlen).map(|_| alphabet[next(alphabet.len())]).collect();
            assert_eq!(
                glob_match(&pat, &text, mode),
                old_glob(&pat, &text, path_mode),
                "pattern {pat:?} text {text:?} path_mode {path_mode}"
            );
        }
    }
}
