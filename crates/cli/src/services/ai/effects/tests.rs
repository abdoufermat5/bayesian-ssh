use super::*;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::types::Analyzer;

const DEMO: &str = "set -euo pipefail; A=/var/tmp/var-www-cleanup-2026-10-24; \
    install -d -m 0750 \"$A/releases\" \"$A/cache/assets\"; \
    mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 /var/www/releases/2026-09-21 \"$A/releases/\"; \
    mv /var/www/cache/assets/bundle-old.js \"$A/cache/assets/\"; echo 'archive complete'";

fn fx(cmd: &str) -> Effects {
    effects_for(cmd, &ShellAnalyzer.analyze(cmd))
}

fn sum(cmd: &str) -> String {
    summary(&fx(cmd))
}

#[test]
fn nginx_signals_read_as_service_changes() {
    assert_eq!(
        sum("sudo nginx -s reload"),
        "reloads nginx · deletes nothing · sudo yes"
    );
    assert_eq!(
        sum("nginx -s quit"),
        "stops nginx · deletes nothing · sudo no"
    );
    let a = ShellAnalyzer.analyze("nginx -t");
    assert!(effects(&a).items.is_empty());
}

#[test]
fn the_demo_plan_reads_as_the_brief_says() {
    let e = fx(DEMO);
    assert_eq!(
        summary(&e),
        "creates 2 directories · moves 4 paths · deletes nothing · sudo no"
    );
    assert_eq!(
        e.items[0].targets,
        vec![
            "/var/tmp/var-www-cleanup-2026-10-24/releases",
            "/var/tmp/var-www-cleanup-2026-10-24/cache/assets"
        ]
    );
    assert_eq!(
        e.items[1].dest.as_deref(),
        Some("/var/tmp/var-www-cleanup-2026-10-24/releases/")
    );
    assert!(!e.deletes && !e.sudo && !e.network);
}

#[test]
fn effects_alone_keeps_variables_as_written() {
    let a = ShellAnalyzer.analyze(DEMO);
    let e = effects(&a);
    assert_eq!(e.items[0].targets[0], "$A/releases");
}

#[test]
fn variables_expand_from_export_braces_and_chains() {
    let e = fx("export A=/srv; B=\"$A/b\"; mkdir ${B}/x $C/y");
    assert_eq!(e.items[0].targets, vec!["/srv/b/x", "$C/y"]);
    let e = fx("A='$HOME'; rm $A");
    assert_eq!(e.items[0].targets, vec!["$HOME"]);
    let e = fx("A=$(pwd); rm $A/x");
    assert_eq!(e.items[0].targets, vec!["$A/x"]);
}

#[test]
fn creating_and_copying() {
    assert_eq!(
        sum("touch /a /b; ln -sf /t /l"),
        "creates 3 paths · deletes nothing · sudo no"
    );
    let e = fx("ln -sf /opt/app-2 /opt/current");
    assert_eq!(e.items[0].targets, vec!["/opt/current"]);
    assert_eq!(e.items[0].dest.as_deref(), Some("/opt/app-2"));
    assert_eq!(
        sum("mkdir -p -m 0700 /a/b"),
        "creates 1 directory · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("cp -r /a /b /dst"),
        "copies 2 paths · deletes nothing · sudo no"
    );
    let s = sum("rsync -a -e ssh /a/ web:/b/");
    assert!(
        s.starts_with("copies 1 path") && s.contains("uses the network"),
        "{s}"
    );
}

#[test]
fn deleting_in_all_its_forms() {
    for cmd in [
        "rm -rf /a /b",
        "rmdir /a",
        "find /tmp -name '*.log' -delete",
        "find /tmp -name '*.log' -exec rm {} \\;",
        "git clean -fdx",
        "git reset --hard HEAD~1",
        "docker rm -f web",
        "docker image rm foo",
        "docker system prune -af",
        "crontab -r",
        "apt-get remove -y nginx",
        "rsync -a --delete /a/ /b/",
        "dd if=/dev/zero of=/dev/sda",
        "userdel bob",
    ] {
        assert!(fx(cmd).deletes, "{cmd}");
        assert!(!summary(&fx(cmd)).contains("deletes nothing"), "{cmd}");
    }
    assert_eq!(sum("rm -rf /a /b"), "deletes 2 paths · sudo no");
    assert_eq!(
        sum("find /tmp -name x -exec rm {} \\;"),
        "deletes files found under /tmp · sudo no"
    );
    assert_eq!(
        sum("docker rm -f web db"),
        "deletes container web, container db · sudo no"
    );
}

#[test]
fn editing_and_devices() {
    assert_eq!(
        sum("sed -i 's/a/b/' /etc/x /etc/y"),
        "edits 2 paths · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("sed -i.bak -e 's/a/b/' /etc/x"),
        "edits 1 path · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("echo x | tee -a /etc/x"),
        "edits 1 path · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("echo x >> /etc/x"),
        "edits 1 path · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("truncate -s 0 /var/log/x"),
        "edits 1 path · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("dd if=a of=/tmp/x"),
        "edits 1 path · deletes nothing · sudo no"
    );
    let s = sum("dd if=a of=/dev/sdb bs=1M");
    assert!(s.starts_with("writes device /dev/sdb"), "{s}");
    assert!(s.contains("may delete data"), "{s}");
    assert_eq!(sum("ls > /dev/null"), "deletes nothing · sudo no");
    assert_eq!(
        sum("echo hi > /tmp/f"),
        "edits 1 path · deletes nothing · sudo no"
    );
}

#[test]
fn a_redirect_does_not_hide_the_command() {
    assert_eq!(
        sum("mv /a /b > /tmp/log"),
        "moves 1 path · edits 1 path · deletes nothing · sudo no"
    );
}

#[test]
fn permissions_and_accounts() {
    assert_eq!(
        sum("chmod -R 750 /srv/app"),
        "changes 1 path · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("chmod -x /a /b"),
        "changes 2 paths · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("chown -R www:www /srv /var/x"),
        "changes 2 paths · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("useradd -m -s /bin/bash bob"),
        "changes user bob · deletes nothing · sudo no"
    );
}

#[test]
fn services_and_processes() {
    assert_eq!(
        sum("sudo -n systemctl restart -- nginx"),
        "restarts nginx · deletes nothing · sudo yes"
    );
    assert_eq!(
        sum("systemctl enable --now nginx.service"),
        "starts nginx.service · enables nginx.service · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("service nginx reload"),
        "reloads nginx · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("systemctl daemon-reload"),
        "reloads systemd · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("kill -9 123 456"),
        "stops pid 123, pid 456 · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("pkill -f nginx"),
        "stops nginx · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("killall -HUP nginx"),
        "reloads nginx · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("docker restart web"),
        "restarts container web · deletes nothing · sudo no"
    );
    assert_eq!(sum("systemctl status nginx"), "deletes nothing · sudo no");
}

#[test]
fn packages_and_mounts() {
    assert_eq!(
        sum("apt-get install -y nginx curl"),
        "installs nginx, curl · deletes nothing · sudo no · uses the network"
    );
    assert_eq!(sum("apt purge -y nginx"), "removes nginx · sudo no");
    assert_eq!(
        sum("pip install requests"),
        "installs requests · deletes nothing · sudo no · uses the network"
    );
    assert_eq!(sum("npm uninstall left-pad"), "removes left-pad · sudo no");
    assert_eq!(
        sum("mount /dev/sdb1 /mnt/data"),
        "mounts /mnt/data · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("umount /mnt/data"),
        "unmounts /mnt/data · deletes nothing · sudo no"
    );
    assert_eq!(
        sum("crontab -e"),
        "edits the crontab · deletes nothing · sudo no"
    );
}

#[test]
fn anything_unrecognised_is_shown() {
    let s = sum("mytool --apply");
    assert!(s.starts_with("runs mytool"), "{s}");
    let s = sum("curl -o /tmp/x https://example.com");
    assert!(s.contains("uses the network"), "{s}");
    assert!(s.contains("/tmp/x") || s.contains("runs curl"), "{s}");
}

#[test]
fn sudo_and_unparsed_commands() {
    assert!(fx("sudo -u deploy ls /").sudo);
    assert!(fx("doas rm /x").sudo);
    assert!(!fx("ls /").sudo);
    let e = fx("eval \"$(echo cm0gLXJm | base64 -d)\"");
    assert!(e.deletes && e.not_understood.is_some());
    let e = fx("sudo bash -c \"$X\"");
    assert!(e.sudo && e.deletes);
    let s = summary(&e);
    assert!(
        s.contains("may delete data") && s.contains("part of it not understood"),
        "{s}"
    );
    assert!(summary(&fx("if then")).contains("part of it not understood"));
}

#[test]
fn summary_orders_and_counts_stably() {
    let e = Effects {
        items: vec![
            Effect {
                verb: Verb::Moves,
                targets: vec!["a".into(), "b".into()],
                dest: Some("d".into()),
            },
            Effect {
                verb: Verb::Creates,
                targets: vec!["c".into()],
                dest: None,
            },
            Effect {
                verb: Verb::Moves,
                targets: vec!["e".into()],
                dest: Some("d".into()),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        summary(&e),
        "creates 1 path · moves 3 paths · deletes nothing · sudo no"
    );
}

#[test]
fn lines_split_where_the_shell_splits() {
    assert_eq!(
        command_lines(DEMO),
        vec![
            "A=/var/tmp/var-www-cleanup-2026-10-24",
            "install -d -m 0750 \"$A/releases\" \"$A/cache/assets\"",
            "mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 /var/www/releases/2026-09-21 \"$A/releases/\"",
            "mv /var/www/cache/assets/bundle-old.js \"$A/cache/assets/\"",
            "echo 'archive complete'",
        ]
    );
    assert_eq!(
        command_lines("echo 'a; b' && ls"),
        vec!["echo 'a; b'", "ls"]
    );
    assert_eq!(
        command_lines("echo \"a && b\" || true"),
        vec!["echo \"a && b\"", "true"]
    );
}

#[test]
fn lines_handle_continuations_pipes_comments_and_set() {
    assert_eq!(
        command_lines("mv a b \\\n   c d/\n# note\nls # trailing\n"),
        vec!["mv a b c d/", "ls"]
    );
    assert_eq!(
        command_lines("ps aux | grep x |\n  wc -l; echo $(a; b)"),
        vec!["ps aux | grep x | wc -l", "echo $(a; b)"]
    );
    assert_eq!(
        command_lines("#!/bin/sh\nset -e\nset -euo pipefail\nset -x\nset +e\nset -o pipefail\nls"),
        vec!["ls"]
    );
    assert_eq!(command_lines("echo ${#x} $#"), vec!["echo ${#x} $#"]);
}

#[test]
fn heredoc_bodies_stay_with_their_command() {
    let script = "cat > /etc/x <<'EOF'\nline; one\n# not a comment\nEOF\nsystemctl reload nginx";
    assert_eq!(
        command_lines(script),
        vec![
            "cat > /etc/x <<'EOF'\nline; one\n# not a comment\nEOF",
            "systemctl reload nginx"
        ]
    );
    let tabbed = "cat <<-END; echo after\n\tbody\n\tEND\nls";
    assert_eq!(
        command_lines(tabbed),
        vec!["cat <<-END; echo after\n\tbody\n\tEND", "ls"]
    );
    let s = sum(script);
    assert!(
        s.contains("edits 1 path") && s.contains("reloads nginx"),
        "{s}"
    );
}
