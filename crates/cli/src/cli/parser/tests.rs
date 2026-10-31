use super::*;
use clap::Parser;

#[test]
fn teams_device_revoke_parses() {
    let id = "6f1c2f64-2a63-4a43-9a43-1f0f0b6f6c11";
    let cli = Cli::try_parse_from(["bssh", "teams", "device", "revoke", id]).unwrap();
    match cli.command {
        Some(Commands::Teams {
            command:
                TeamsCommands::Device {
                    command: TeamsDeviceCommands::Revoke { device_id },
                },
        }) => assert_eq!(device_id.to_string(), id),
        _ => panic!("expected `teams device revoke`"),
    }
    assert!(Cli::try_parse_from(["bssh", "teams", "device", "revoke"]).is_err());
    assert!(Cli::try_parse_from(["bssh", "teams", "device", "revoke", "nope"]).is_err());
}

#[test]
fn teams_audit_limit_and_all_exclude_each_other() {
    assert!(Cli::try_parse_from(["bssh", "teams", "audit", "t", "--limit", "500"]).is_ok());
    assert!(Cli::try_parse_from(["bssh", "teams", "audit", "t", "--all"]).is_ok());
    assert!(Cli::try_parse_from(["bssh", "teams", "audit", "t", "--all", "--limit", "5"]).is_err());
}

#[test]
fn org_commands_parse() {
    assert!(Cli::try_parse_from(["bssh", "org", "list"]).is_ok());
    assert!(Cli::try_parse_from(["bssh", "org", "show"]).is_ok());
    assert!(Cli::try_parse_from(["bssh", "org", "show", "platform"]).is_ok());
    assert!(Cli::try_parse_from(["bssh", "org", "create"]).is_ok());
    assert!(
        Cli::try_parse_from(["bssh", "org", "owners", "add", "platform", "bo@example.com"]).is_ok()
    );
    assert!(Cli::try_parse_from([
        "bssh",
        "org",
        "owners",
        "remove",
        "platform",
        "bo@example.com"
    ])
    .is_ok());
    assert!(Cli::try_parse_from(["bssh", "teams", "create", "infra", "--org", "platform"]).is_ok());

    let cli =
        Cli::try_parse_from(["bssh", "org", "billing", "--checkout", "--seats", "8"]).unwrap();
    match cli.command {
        Some(Commands::Org {
            command:
                OrgCommands::Billing {
                    org,
                    checkout,
                    seats,
                    yearly,
                    manage,
                },
        }) => {
            assert_eq!(org, None);
            assert!(checkout);
            assert_eq!(seats, Some(8));
            assert!(!yearly);
            assert!(!manage);
        }
        _ => panic!("expected `org billing`"),
    }
    // `--manage` and `--checkout` are alternatives, and seats need checkout.
    assert!(Cli::try_parse_from(["bssh", "org", "billing", "--manage", "--checkout"]).is_err());
    assert!(Cli::try_parse_from(["bssh", "org", "billing", "--seats", "4"]).is_err());
}

#[test]
fn connect_without_port_keeps_stored_port() {
    // A clap default here would silently override the saved port.
    let cli = Cli::try_parse_from(["bssh", "connect", "db01"]).unwrap();
    match cli.command {
        Some(Commands::Connect { port, .. }) => assert_eq!(port, None),
        _ => unreachable!(),
    }
}

#[test]
fn port_zero_rejected() {
    assert!(Cli::try_parse_from(["bssh", "connect", "db01", "-p", "0"]).is_err());
    assert!(Cli::try_parse_from(["bssh", "add", "n", "h", "-p", "0"]).is_err());
}

#[test]
fn teams_init_accepts_email_code() {
    let cli = Cli::try_parse_from([
        "bssh",
        "teams",
        "init",
        "--server",
        "https://teams.example.com",
        "--email",
        "ana@example.com",
        "--email-code",
        "123456",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Teams {
            command:
                TeamsCommands::Init {
                    server,
                    email,
                    email_code,
                    ..
                },
        }) => {
            assert_eq!(server, "https://teams.example.com");
            assert_eq!(email, "ana@example.com");
            assert_eq!(email_code.as_deref(), Some("123456"));
        }
        _ => unreachable!(),
    }
    // Optional: setup without a code still parses.
    let cli = Cli::try_parse_from([
        "bssh",
        "teams",
        "init",
        "--server",
        "https://teams.example.com",
        "--email",
        "ana@example.com",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Teams {
            command: TeamsCommands::Init { email_code, .. },
        }) => assert_eq!(email_code, None),
        _ => unreachable!(),
    }
}

#[test]
fn forward_agent_flags_parse() {
    let cli = Cli::try_parse_from(["bssh", "add", "n", "h", "--forward-agent"]).unwrap();
    match cli.command {
        Some(Commands::Add {
            forward_agent,
            no_forward_agent,
            ..
        }) => {
            assert!(forward_agent);
            assert!(!no_forward_agent);
        }
        _ => panic!("expected `add`"),
    }

    let cli = Cli::try_parse_from(["bssh", "edit", "n", "--no-forward-agent"]).unwrap();
    match cli.command {
        Some(Commands::Edit {
            forward_agent,
            no_forward_agent,
            ..
        }) => {
            assert!(!forward_agent);
            assert!(no_forward_agent);
        }
        _ => panic!("expected `edit`"),
    }

    assert!(Cli::try_parse_from([
        "bssh",
        "add",
        "n",
        "h",
        "--forward-agent",
        "--no-forward-agent"
    ])
    .is_err());
    assert!(Cli::try_parse_from(["bssh", "confirm", "db-prod"]).is_ok());
}
