//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

use crate::config::HostKeyPolicy;
use crate::models::Connection;
use std::path::PathBuf;

use super::argv::{
    ArgvExtras, HopOptions, ShellExtras, jump_proxy_command, ssh_strict_host_key_value,
};
use super::interactive::{
    AuthPrompt, auth_prompt_kind, cert_through_kerberos_bastion, extract_between_markers,
    interactive_payload, needs_tty, prompt_error,
};
use super::process::{classify_ssh_failure, run_argv_limited, write_cert_files};
use super::*;
use crate::services::transport::types::{CertAuth, ExecOptions, LimitedOutput};

fn c(kerb: bool, bastion: Option<&str>, key: Option<&str>) -> Connection {
    Connection {
        bastion: bastion.map(String::from),
        use_kerberos: kerb,
        key_path: key.map(String::from),
        ..Connection::new("x".into(), "target.example".into(), "alice".into(), 2222)
    }
}

#[test]
fn argv_simple() {
    let argv = SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "accept-new");
    assert_eq!(argv[0], "ssh");
    assert!(argv.contains(&"-p".to_string()));
    assert!(argv.contains(&"2222".to_string()));
    assert!(argv.contains(&"alice@target.example".to_string()));
    assert_eq!(argv.last().unwrap(), "uptime");
    // no -K, no -i, no -J
    assert!(!argv.contains(&"-K".to_string()));
    assert!(!argv.contains(&"-i".to_string()));
    assert!(!argv.contains(&"-J".to_string()));
}

#[test]
fn exec_argv_never_forces_tty() {
    for bastion in [None, Some("b.example")] {
        let argv =
            SubprocessTransport::build_exec_argv(&c(false, bastion, None), "uptime", "accept-new");
        assert!(argv.contains(&"-T".to_string()));
        assert!(!argv.contains(&"-t".to_string()));
        assert!(!argv.contains(&"-tt".to_string()));
        assert!(!argv.iter().any(|a| a.contains("RequestTTY=force")));
        assert!(!argv.iter().any(|a| a.contains(" -tt")));
    }
}

#[test]
fn argv_kerberos_adds_k_flag() {
    let argv = SubprocessTransport::build_exec_argv(&c(true, None, None), "uptime", "accept-new");
    assert!(argv.contains(&"-K".to_string()));
}

#[test]
fn argv_bastion_uses_proxy_command() {
    let argv = SubprocessTransport::build_exec_argv(
        &c(false, Some("b.example"), None),
        "uptime",
        "accept-new",
    );
    assert!(argv.iter().any(|a| a.contains("ProxyCommand=")));
    assert!(argv.iter().any(|a| a.contains("alice@b.example")));
}

#[test]
fn argv_kerberos_bastion_uses_proxy_command() {
    let argv = SubprocessTransport::build_exec_argv(
        &c(true, Some("b.example"), None),
        "ls -l /tmp",
        "accept-new",
    );
    assert!(argv.iter().any(|a| a.contains("ProxyCommand=")));
    assert!(argv.contains(&"-K".to_string()));
    assert!(argv.iter().any(|a| a.contains("alice@b.example")));
    assert!(argv.contains(&"ls -l /tmp".to_string()));
}

#[test]
fn argv_key_path_uses_i_flag() {
    let argv = SubprocessTransport::build_exec_argv(
        &c(false, None, Some("/k/id_ed25519")),
        "uptime",
        "accept-new",
    );
    assert!(argv.contains(&"-i".to_string()));
    assert!(argv.contains(&"/k/id_ed25519".to_string()));
}

#[test]
fn argv_shkc_threads_config_value() {
    let argv = SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "strict");
    assert!(argv.iter().any(|a| a == "StrictHostKeyChecking=yes"));
    let argv_off = SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "off");
    assert!(argv_off.iter().any(|a| a == "StrictHostKeyChecking=no"));
}

#[test]
fn ssh_shkc_value_accepts_ssh_aliases_and_fails_closed() {
    assert_eq!(ssh_strict_host_key_value("strict"), "yes");
    assert_eq!(ssh_strict_host_key_value("YES"), "yes");
    assert_eq!(ssh_strict_host_key_value("accept-new"), "accept-new");
    assert_eq!(ssh_strict_host_key_value("off"), "no");
    assert_eq!(ssh_strict_host_key_value("no"), "no");
    // A typo must not degrade to accept-new (which trusts an unknown host).
    assert_eq!(ssh_strict_host_key_value("accept_new"), "yes");
}

#[test]
fn shell_argv_simple() {
    let argv =
        SubprocessTransport::build_shell_argv(&c(false, None, None), HostKeyPolicy::AcceptNew);
    assert_eq!(argv[0], "ssh");
    assert!(argv.contains(&"-p".to_string()));
    assert!(argv.contains(&"2222".to_string()));
    assert!(argv.last().unwrap().contains("alice@target.example"));
}

#[test]
fn shell_argv_kerberos_adds_flags() {
    let argv =
        SubprocessTransport::build_shell_argv(&c(true, None, None), HostKeyPolicy::AcceptNew);
    assert!(argv.contains(&"-tt".to_string()));
    assert!(argv.contains(&"-K".to_string()));
    // Agent forwarding is a per-connection opt-in, not a Kerberos side effect.
    assert!(!argv.contains(&"-A".to_string()));
}

#[test]
fn shell_argv_forwards_the_agent_only_when_opted_in() {
    let mut conn = c(true, None, None);
    conn.forward_agent = true;
    let argv = SubprocessTransport::build_shell_argv(&conn, HostKeyPolicy::AcceptNew);
    let a = argv.iter().position(|x| x == "-A");
    let k = argv.iter().position(|x| x == "-K");
    assert!(a.is_some() && k.is_some() && a < Some(k.unwrap()));

    // Without Kerberos, -A alone still needs the opt-in.
    let plain =
        SubprocessTransport::build_shell_argv(&c(false, None, None), HostKeyPolicy::AcceptNew);
    assert!(!plain.contains(&"-A".to_string()));

    let mut keyed = c(false, None, None);
    keyed.forward_agent = true;
    let argv = SubprocessTransport::build_shell_argv(&keyed, HostKeyPolicy::AcceptNew);
    assert!(argv.contains(&"-A".to_string()));
    assert!(!argv.contains(&"-K".to_string()));
}

#[test]
fn exec_argv_never_forwards_the_agent() {
    for forward_agent in [false, true] {
        let mut conn = c(true, None, None);
        conn.forward_agent = forward_agent;
        let argv = SubprocessTransport::build_exec_argv(&conn, "uptime", "accept-new");
        assert!(!argv.contains(&"-A".to_string()), "{argv:?}");
        assert!(argv.contains(&"-K".to_string()));
    }
}

#[test]
fn interactive_bastion_uses_the_stored_port() {
    let plain = SubprocessTransport::build_shell_argv(
        &c(true, Some("b.example"), None),
        HostKeyPolicy::AcceptNew,
    );
    assert!(!plain.contains(&"-p".to_string()), "{plain:?}");
    assert!(plain.contains(&"alice@b.example".to_string()));

    let ported = SubprocessTransport::build_shell_argv(
        &c(true, Some("b.example:2222"), None),
        HostKeyPolicy::AcceptNew,
    );
    let p = ported.iter().position(|a| a == "-p").expect("-p present");
    assert_eq!(ported[p + 1], "2222");
    assert!(
        ported.contains(&"alice@b.example".to_string()),
        "{ported:?}"
    );
}

/// The bastion hop is the one bssh itself verifies, so it must follow the
/// configured policy instead of a hard-coded `accept-new`.
#[test]
fn shell_argv_bastion_hop_honours_the_configured_policy() {
    let conn = c(false, Some("b.example"), None);
    let expected = [
        (HostKeyPolicy::Strict, "StrictHostKeyChecking=yes"),
        (HostKeyPolicy::AcceptNew, "StrictHostKeyChecking=accept-new"),
        (HostKeyPolicy::Off, "StrictHostKeyChecking=no"),
    ];
    for (policy, option) in expected {
        let argv = SubprocessTransport::build_shell_argv(&conn, policy);
        let proxy = argv
            .iter()
            .find(|a| a.starts_with("ProxyCommand="))
            .expect("ProxyCommand present");
        assert!(proxy.contains(option), "{policy:?}: {proxy}");
        // The target prompt is still the user's ssh's (documented behaviour).
        assert!(
            !argv.iter().any(|a| a == option),
            "{policy:?} leaks to target"
        );
    }
}

#[test]
fn shell_argv_direct_connection_leaves_target_prompt_to_ssh() {
    // No bastion: nothing is enforced, OpenSSH's interactive `ask` applies.
    for policy in [
        HostKeyPolicy::Strict,
        HostKeyPolicy::AcceptNew,
        HostKeyPolicy::Off,
    ] {
        let argv = SubprocessTransport::build_shell_argv(&c(false, None, None), policy);
        assert!(
            !argv.iter().any(|a| a.starts_with("StrictHostKeyChecking=")),
            "{policy:?}: {argv:?}"
        );
    }
}

#[test]
fn destination_follows_double_dash() {
    let mut conn = c(false, None, None);
    conn.user = "-oProxyCommand=touch /tmp/pwned".into();
    let argv = SubprocessTransport::build_exec_argv(&conn, "uptime", "accept-new");
    let dd = argv.iter().position(|a| a == "--").expect("-- present");
    assert!(argv[dd + 1].starts_with("-oProxyCommand"));
    let shell = SubprocessTransport::build_shell_argv(&conn, HostKeyPolicy::AcceptNew);
    let dd = shell.iter().position(|a| a == "--").expect("-- present");
    assert_eq!(dd, shell.len() - 2);
}

#[test]
fn proxy_command_never_expands_unsafe_host() {
    let mut conn = c(false, Some("b.example"), None);
    conn.host = "x$(touch /tmp/pwned)".into();
    let argv = SubprocessTransport::build_exec_argv(&conn, "uptime", "accept-new");
    let proxy = argv
        .iter()
        .find(|a| a.starts_with("ProxyCommand="))
        .unwrap();
    assert!(!proxy.contains("%h"));
    assert!(proxy.contains("'x$(touch /tmp/pwned):2222'"));
    assert!(proxy.ends_with("-- alice@b.example"));

    // Shell-inert hosts keep `%h` so ~/.ssh/config HostName aliases work.
    let argv = SubprocessTransport::build_exec_argv(
        &c(false, Some("b.example"), None),
        "uptime",
        "accept-new",
    );
    let proxy = argv
        .iter()
        .find(|a| a.starts_with("ProxyCommand="))
        .unwrap();
    assert!(proxy.contains("-W %h:%p"));
}

#[test]
fn proxy_command_escapes_percent() {
    let mut conn = c(false, Some("b.example"), Some("/keys/100%key"));
    conn.bastion_user = Some("ops".into());
    let proxy = jump_proxy_command(&conn, "", "accept-new", &HopOptions::default());
    assert!(proxy.contains("-i /keys/100%%key"));
}

#[test]
fn hop_argv_is_batch_and_bounded_when_non_interactive() {
    let conn = c(false, Some("b.example"), Some("/keys/id_ed25519"));
    let hop = HopOptions {
        non_interactive: true,
        connect_timeout_secs: Some(9),
    };
    let proxy = jump_proxy_command(&conn, " -T", "accept-new", &hop);
    assert!(proxy.contains("-o BatchMode=yes"), "{proxy}");
    assert!(proxy.contains("-o ConnectTimeout=9"), "{proxy}");
    // The hop logs in as the connection's own user with its own key.
    assert!(proxy.contains("-i /keys/id_ed25519"), "{proxy}");
    assert!(proxy.ends_with("-- alice@b.example"), "{proxy}");

    // Interactive CLI hops keep prompting: no batch switches.
    let cli = jump_proxy_command(&conn, "", "accept-new", &HopOptions::default());
    assert!(!cli.contains("BatchMode"), "{cli}");
    assert!(!cli.contains("ConnectTimeout"), "{cli}");
}

#[test]
fn cert_hops_keep_the_original_bastion_identity() {
    // `exec.rs::agent_connection` swaps the target login to bssh-agent and keeps the
    // original bastion user (and the original key for the hop only).
    let mut conn = c(
        false,
        Some("b.example"),
        Some("/home/alice/.ssh/id_ed25519"),
    );
    conn.bastion_user = Some("ops".into());
    conn.user = "bssh-agent".into();
    let hop = HopOptions {
        non_interactive: true,
        connect_timeout_secs: Some(5),
    };
    let proxy = jump_proxy_command(&conn, " -T", "accept-new", &hop);
    assert!(proxy.ends_with("-- ops@b.example"), "{proxy}");

    // Without an explicit bastion user the raw helper falls back to `conn.user`;
    // exec.rs sets `bastion_user` before it calls this, which the exec test pins.
    conn.bastion_user = None;
    conn.user = "bssh-agent".into();
    let proxy = jump_proxy_command(&conn, " -T", "accept-new", &hop);
    assert!(proxy.ends_with("-- bssh-agent@b.example"), "{proxy}");
}

#[test]
fn cert_exec_argv_keeps_the_hop_key_and_offers_only_the_certificate() {
    let mut conn = c(
        false,
        Some("b.example"),
        Some("/home/alice/.ssh/id_ed25519"),
    );
    conn.bastion_user = Some("alice".into());
    let extras = ArgvExtras {
        cert: Some((PathBuf::from("/t/id"), PathBuf::from("/t/id-cert.pub"))),
        ..batch_extras()
    };
    let argv = SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &extras);
    let proxy = argv
        .iter()
        .find(|a| a.starts_with("ProxyCommand="))
        .expect("ProxyCommand");
    assert!(proxy.contains("-i /home/alice/.ssh/id_ed25519"), "{proxy}");
    assert!(proxy.contains("BatchMode=yes"), "{proxy}");
    assert!(proxy.ends_with("-- alice@b.example"), "{proxy}");
    // The target still authenticates with the certificate only.
    let target_keys: Vec<&String> = argv
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "-i")
        .map(|(i, _)| &argv[i + 1])
        .collect();
    assert_eq!(target_keys, vec!["/t/id"]);
}

fn batch_extras() -> ArgvExtras {
    ArgvExtras {
        non_interactive: true,
        connect_timeout_secs: Some(7),
        cert: None,
        control_dir: None,
    }
}

#[test]
fn default_exec_argv_has_no_batch_options() {
    let argv = SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "accept-new");
    assert!(!argv.iter().any(|a| a.starts_with("BatchMode")));
    assert!(!argv.iter().any(|a| a.starts_with("ConnectTimeout")));
    assert!(!argv.contains(&"-a".to_string()));
}

#[test]
fn batch_argv_never_prompts_and_never_forwards() {
    let argv = SubprocessTransport::build_exec_argv_with(
        &c(false, None, None),
        "uptime",
        "accept-new",
        &batch_extras(),
    );
    let has = |s: &str| argv.iter().any(|a| a == s);
    assert!(has("BatchMode=yes"));
    assert!(has("ConnectTimeout=7"));
    assert!(has("ClearAllForwardings=yes"));
    assert!(has("-a") && has("-x") && has("-T"));
    // The command is still the last word, after `--` and the destination.
    let n = argv.len();
    assert_eq!(argv[n - 1], "uptime");
    assert_eq!(argv[n - 2], "alice@target.example");
    assert_eq!(argv[n - 3], "--");
}

#[test]
fn cert_argv_offers_only_the_certificate() {
    let extras = ArgvExtras {
        cert: Some((PathBuf::from("/t/id"), PathBuf::from("/t/id-cert.pub"))),
        ..batch_extras()
    };
    // A per-connection key must not be offered next to the certificate.
    let argv = SubprocessTransport::build_exec_argv_with(
        &c(false, None, Some("/home/alice/.ssh/id_rsa")),
        "id",
        "accept-new",
        &extras,
    );
    assert!(argv.contains(&"IdentitiesOnly=yes".to_string()));
    assert!(argv.contains(&"CertificateFile=/t/id-cert.pub".to_string()));
    let keys: Vec<&String> = argv
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "-i")
        .map(|(i, _)| &argv[i + 1])
        .collect();
    assert_eq!(keys, vec!["/t/id"]);
}

fn out(exit: Option<i32>, stderr: &str) -> LimitedOutput {
    LimitedOutput {
        stderr: stderr.as_bytes().to_vec(),
        exit_code: exit,
        ..LimitedOutput::default()
    }
}

#[test]
fn permission_denied_becomes_an_actionable_error() {
    let err = classify_ssh_failure(
        &out(
            Some(255),
            "alice@host: Permission denied (publickey,password).\n",
        ),
        None,
    )
    .expect("ssh failure")
    .to_string();
    assert!(
        err.contains("needs interactive auth; use a key or ssh-agent"),
        "{err}"
    );
    assert!(err.contains("Permission denied"), "{err}");
}

#[test]
fn a_refused_bastion_login_names_the_bastion() {
    let mut conn = c(false, Some("bastion.corp"), None);
    conn.bastion_user = Some("alice".into());
    let err = classify_ssh_failure(
        &out(
            Some(255),
            "alice@bastion.corp: Permission denied (publickey).\n",
        ),
        Some(&conn),
    )
    .expect("ssh failure")
    .to_string();
    assert!(
        err.contains("the bastion bastion.corp refused the login"),
        "{err}"
    );
    assert!(err.contains("alice@bastion.corp"), "{err}");
    assert!(!err.contains("needs interactive auth"), "{err}");

    // The target itself refusing a key keeps the key/agent advice.
    let err = classify_ssh_failure(
        &out(
            Some(255),
            "deploy@target.example: Permission denied (publickey).\n",
        ),
        Some(&conn),
    )
    .expect("ssh failure")
    .to_string();
    assert!(err.contains("needs interactive auth"), "{err}");
}

#[test]
fn a_bastion_that_forbids_forwarding_suggests_an_interactive_bastion() {
    let mut conn = c(false, Some("bastion.corp"), None);
    conn.name = "web-1".into();
    let err = classify_ssh_failure(
        &out(
            Some(255),
            "channel 0: open failed: administratively prohibited: open failed\n\
                 Connection closed by UNKNOWN port 65535\n",
        ),
        Some(&conn),
    )
    .expect("ssh failure")
    .to_string();
    assert!(err.contains("forbids TCP forwarding"), "{err}");
    assert!(err.contains("bssh edit web-1 --kerberos true"), "{err}");
}

#[test]
fn a_gssapi_rejection_points_at_the_ticket() {
    let mut conn = c(true, Some("bastion.corp"), None);
    conn.bastion_user = Some("alice".into());
    let err = classify_ssh_failure(
        &out(
            Some(255),
            "alice@bastion.corp: Permission denied (gssapi-with-mic,publickey).\n",
        ),
        Some(&conn),
    )
    .expect("ssh failure")
    .to_string();
    assert!(err.contains("Kerberos login failed"), "{err}");
    assert!(err.contains("klist") && err.contains("kinit"), "{err}");
}

#[test]
fn other_ssh_failures_keep_their_message() {
    let err = classify_ssh_failure(
        &out(
            Some(255),
            "ssh: connect to host h port 22: Connection refused\n",
        ),
        None,
    )
    .expect("ssh failure")
    .to_string();
    assert!(err.contains("Connection refused"), "{err}");
}

#[test]
fn remote_failures_are_results_not_errors() {
    // The remote command's own exit codes are data, even 255 without an ssh message.
    assert!(classify_ssh_failure(&out(Some(1), "Permission denied\n"), None).is_none());
    assert!(classify_ssh_failure(&out(Some(255), "boom\n"), None).is_none());
    assert!(classify_ssh_failure(&out(Some(0), ""), None).is_none());
}

#[test]
fn auth_prompts_are_detected_case_insensitively() {
    assert_eq!(
        auth_prompt_kind("alice@bastion's Password: "),
        Some(AuthPrompt::Password)
    );
    assert_eq!(
        auth_prompt_kind("PASSCODE (or press Ctrl-C):"),
        Some(AuthPrompt::OneTimeCode)
    );
    assert_eq!(
        auth_prompt_kind("Enter verification code: "),
        Some(AuthPrompt::OneTimeCode)
    );
    assert_eq!(
        auth_prompt_kind("YubiKey OTP: "),
        Some(AuthPrompt::OneTimeCode)
    );
    assert_eq!(
        auth_prompt_kind("One-time password: "),
        Some(AuthPrompt::OneTimeCode)
    );
    assert_eq!(
        auth_prompt_kind("MFA required"),
        Some(AuthPrompt::OneTimeCode)
    );
    assert_eq!(
        auth_prompt_kind("2FA token:"),
        Some(AuthPrompt::OneTimeCode)
    );
    assert_eq!(
        auth_prompt_kind("Enter PIN for Yubikey:"),
        Some(AuthPrompt::OneTimeCode)
    );
    // A banner or MOTD is not a prompt.
    assert_eq!(
        auth_prompt_kind("Welcome to bastion corp. Use of this system is logged.\n$ "),
        None
    );
}

#[test]
fn the_prompt_error_names_the_bastion_and_the_way_out() {
    let err = prompt_error(
        &c(true, Some("bastion.corp"), None),
        AuthPrompt::OneTimeCode,
    )
    .to_string();
    assert!(
        err.contains("the bastion bastion.corp asks for a one-time code"),
        "{err}"
    );
    assert!(err.contains("agents cannot answer prompts"), "{err}");
    assert!(err.contains("key, ssh-agent or a Kerberos ticket"), "{err}");
}

#[test]
fn a_certificate_through_a_kerberos_bastion_explains_why_it_cannot_pass() {
    let mut conn = c(true, Some("bastion.corp"), None);
    conn.name = "web-1".into();
    let err = cert_through_kerberos_bastion(&conn).to_string();
    assert!(err.contains("web-1 is behind a Kerberos bastion"), "{err}");
    assert!(
        err.contains("certificate (bssh-agent) logins cannot pass"),
        "{err}"
    );
    assert!(err.contains("run as your user"), "{err}");
}

#[test]
fn shell_argv_for_an_agent_on_an_interactive_bastion_carries_target_credentials() {
    // Without forward_agent: BatchMode, no -A, but TGT delegation so the
    // bastion can GSSAPI-auth to the target.
    let conn = c(true, Some("bastion.corp"), None);
    let extras = ShellExtras {
        non_interactive: true,
        connect_timeout_secs: Some(12),
    };
    let argv =
        SubprocessTransport::build_shell_argv_ext(&conn, HostKeyPolicy::AcceptNew, true, &extras);
    let has = |s: &str| argv.iter().any(|a| a == s);
    assert!(has("BatchMode=yes"), "{argv:?}");
    assert!(has("ConnectTimeout=12"), "{argv:?}");
    assert!(has("GSSAPIAuthentication=yes"), "{argv:?}");
    assert!(has("GSSAPIDelegateCredentials=yes"), "{argv:?}");
    assert!(!has("-K"), "flag form: {argv:?}");
    assert!(!has("-A"), "forward_agent is off: {argv:?}");
    assert!(has("ClearAllForwardings=yes"), "{argv:?}");
    let dd = argv.iter().position(|a| a == "--").expect("--");
    assert_eq!(argv[dd + 1], "alice@bastion.corp");
    assert_eq!(argv[dd + 2], "alice@target.example");

    // With forward_agent: -A is required when the target wants the user's key.
    let mut fwd = conn;
    fwd.forward_agent = true;
    let argv =
        SubprocessTransport::build_shell_argv_ext(&fwd, HostKeyPolicy::AcceptNew, true, &extras);
    assert!(argv.iter().any(|a| a == "-A"), "{argv:?}");
    assert!(
        !argv.iter().any(|a| a == "ClearAllForwardings=yes"),
        "{argv:?}"
    );
    assert!(argv.iter().any(|a| a == "GSSAPIDelegateCredentials=yes"));
}

#[test]
fn direct_kerberos_exec_argv_does_not_delegate() {
    let conn = c(true, None, None);
    let argv =
        SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &batch_extras());
    assert!(!argv.contains(&"-K".to_string()), "{argv:?}");
    assert!(argv.contains(&"GSSAPIAuthentication=yes".to_string()));
    assert!(argv.contains(&"GSSAPIDelegateCredentials=no".to_string()));
    // Interactive CLI exec keeps `-K` (a human may be watching).
    let cli = SubprocessTransport::build_exec_argv(&conn, "id", "accept-new");
    assert!(cli.contains(&"-K".to_string()), "{cli:?}");
}

#[test]
fn control_master_is_only_for_batch_calls_without_a_certificate() {
    let conn = c(false, Some("b.example"), None);
    let dir = PathBuf::from("/run/bssh/ssh");
    let extras = ArgvExtras {
        control_dir: Some(dir.clone()),
        ..batch_extras()
    };
    let argv = SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &extras);
    assert!(argv.contains(&"ControlMaster=auto".to_string()), "{argv:?}");
    assert!(argv.contains(&"ControlPath=/run/bssh/ssh/%C".to_string()));
    assert!(argv.contains(&"ControlPersist=60".to_string()));

    // Certificates are one identity per command: sharing would reuse the wrong one.
    let cert = ArgvExtras {
        cert: Some((PathBuf::from("/t/id"), PathBuf::from("/t/id-cert.pub"))),
        ..batch_extras()
    };
    let argv = SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &cert);
    assert!(
        !argv.iter().any(|a| a.starts_with("ControlMaster")),
        "{argv:?}"
    );

    // A plain CLI exec never shares connections behind the user's back.
    let cli = SubprocessTransport::build_exec_argv(&conn, "id", "accept-new");
    assert!(
        !cli.iter().any(|a| a.starts_with("ControlMaster")),
        "{cli:?}"
    );
}

#[test]
fn the_echoed_payload_never_contains_a_marker() {
    let p = interactive_payload("BSSH_00ff", "uptime");
    assert!(!p.contains("BSSH_00ff_START"), "{p}");
    assert!(!p.contains("BSSH_00ff_END"), "{p}");
    // One logical command, terminated once: the shell reads it all before running it.
    assert_eq!(p.matches('\n').count(), 2, "{p}");
    assert!(p.contains("{ uptime\n}"), "{p}");
}

#[test]
fn only_output_between_the_markers_is_kept() {
    // Shaped like qupload01v: zsh with a coloured prompt, PROMPT_SP, and the line
    // editor echoing the whole payload before it runs.
    let raw = "banner\r\n\x1b[1m%\x1b[0m   \r \r\r<16:29>[cs-x:~]\x1b[0m$ u\x08uptime; echo 'M''_END'\r\r\n\
                   M_START\r\n 16:29:20 up 26 days\r\nM_END\r\n\x1b[1m%\x1b[0m  <16:29>[cs-x:~]$ \r\n";
    let (lines, start, end) = extract_between_markers(raw, "M_START", "M_END");
    assert!(start && end);
    assert_eq!(lines, vec![" 16:29:20 up 26 days"]);

    // Output with no final newline shares its line with the END marker.
    let raw = "M_START\r\nline one\r\naGk=M_END\r\n";
    let (lines, _, _) = extract_between_markers(raw, "M_START", "M_END");
    assert_eq!(lines, vec!["line one", "aGk="]);

    // No START marker: nothing is output (the caller turns this into an error).
    let (lines, start, _) = extract_between_markers("banner\r\nbye\r\n", "M_START", "M_END");
    assert!(!start && lines.is_empty());
}

#[test]
fn needs_tty_detects_the_ssh_messages() {
    assert!(needs_tty(b"", b"sudo: you must have a pseudo-terminal"));
    assert!(needs_tty(b"sudo: option -t required", b""));
    assert!(!needs_tty(b"hello", b"world"));
}

#[cfg(unix)]
mod run {
    use super::*;
    use std::time::{Duration, Instant};

    fn sh(script: &str, opts: &ExecOptions) -> LimitedOutput {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(run_argv_limited(
            "sh",
            &["-c".to_string(), script.to_string()],
            opts,
            None,
        ))
        .expect("run")
    }

    #[test]
    fn collects_output_and_exit_code() {
        let out = sh("echo out; echo err >&2; exit 3", &ExecOptions::default());
        assert_eq!(out.stdout, b"out\n");
        assert_eq!(out.stderr, b"err\n");
        assert_eq!(out.exit_code, Some(3));
        assert!(!out.truncated && !out.timed_out);
    }

    #[test]
    fn timeout_kills_the_child_and_keeps_partial_output() {
        let opts = ExecOptions {
            timeout: Some(Duration::from_millis(400)),
            ..ExecOptions::default()
        };
        let started = Instant::now();
        let out = sh("echo partial; exec sleep 30", &opts);
        assert!(started.elapsed() < Duration::from_secs(10), "did not wait");
        assert!(out.timed_out);
        assert_eq!(out.exit_code, None);
        assert_eq!(out.stdout, b"partial\n");
    }

    #[test]
    fn timeout_leaves_no_child_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pidfile = dir.path().join("pid");
        let script = format!("echo $$ > {}; exec sleep 30", pidfile.display());
        let opts = ExecOptions {
            timeout: Some(Duration::from_millis(500)),
            ..ExecOptions::default()
        };
        let out = sh(&script, &opts);
        assert!(out.timed_out);
        let pid = std::fs::read_to_string(&pidfile)
            .expect("pid file")
            .trim()
            .to_string();
        // The process is gone (or a zombie already reaped): /proc has no live entry.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        assert!(
            stat.is_empty() || stat.contains(") Z "),
            "still running: {stat}"
        );
    }

    #[test]
    fn caps_each_stream_and_marks_truncated() {
        let opts = ExecOptions {
            max_output_bytes: Some(100),
            ..ExecOptions::default()
        };
        // 200 KB on stdout, 50 bytes on stderr: only stdout exceeds the cap.
        let out = sh(
            "head -c 200000 /dev/zero | tr '\\0' x; echo 'short error' >&2; exit 0",
            &opts,
        );
        assert_eq!(out.stdout.len(), 100);
        assert!(out.stdout.iter().all(|b| *b == b'x'));
        assert_eq!(out.stderr, b"short error\n");
        assert!(out.truncated);
        assert_eq!(
            out.exit_code,
            Some(0),
            "the command was drained, not killed"
        );
    }

    #[test]
    fn output_under_the_cap_is_not_truncated() {
        let opts = ExecOptions {
            max_output_bytes: Some(100),
            ..ExecOptions::default()
        };
        let out = sh("printf 'x%.0s' $(seq 1 100)", &opts);
        assert_eq!(out.stdout.len(), 100);
        assert!(!out.truncated, "exactly the cap fits");
    }

    #[test]
    fn missing_program_is_an_error() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let res = rt.block_on(run_argv_limited(
            "/nonexistent/ssh-binary",
            &[],
            &ExecOptions::default(),
            None,
        ));
        assert!(res.is_err());
    }

    #[test]
    fn cert_files_are_private_and_removed() {
        use std::os::unix::fs::PermissionsExt;
        let cert = CertAuth {
            private_key_openssh: zeroize::Zeroizing::new("KEY".to_string()),
            certificate_openssh: "CERT".to_string(),
        };
        let (dir, key, crt) = write_cert_files(&cert).expect("write");
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode();
        assert_eq!(mode(&key) & 0o077, 0, "key readable by others");
        assert_eq!(mode(dir.path()) & 0o077, 0, "dir readable by others");
        assert_eq!(std::fs::read_to_string(&crt).unwrap(), "CERT");
        let root = dir.path().to_path_buf();
        drop(dir);
        assert!(!root.exists());
    }
}
