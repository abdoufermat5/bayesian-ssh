mod commands;
mod kerberos;
mod tray;

use commands::{
    add_connection, add_key_to_agent, check_popout_main_overlap, claim_popout_session,
    close_all_ptys, close_pty, complete_onboarding, count_active_sessions, create_environment,
    detach_pty, dock_popout_session, edit_connection, focus_terminal_window, get_active_env,
    get_agent_status, get_connections, get_history, get_stats, get_workspace_info,
    import_ssh_config, list_detached_sessions, list_environments, list_popout_sessions,
    load_desktop_settings, needs_onboarding, open_terminal_window, pick_key_file,
    pick_ssh_config_file, quit_app, force_quit_app, reattach_pty, remove_connection, remove_environment,
    resize_pty, save_desktop_settings, save_workspace_config, seal_session_ui, set_active_env,
    spawn_pty, start_agent, write_pty, PtyState,
};
use kerberos::{acquire_kerberos_ticket, get_kerberos_status, renew_kerberos_ticket};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(unix)]
    init_shell_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(PtyState {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_background_color(Some(tauri::window::Color(9, 9, 11, 255)));
            }
            tray::setup_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_active_env,
            set_active_env,
            list_environments,
            create_environment,
            remove_environment,
            get_connections,
            add_connection,
            edit_connection,
            remove_connection,
            get_stats,
            get_history,
            pick_key_file,
            spawn_pty,
            write_pty,
            resize_pty,
            seal_session_ui,
            detach_pty,
            list_detached_sessions,
            list_popout_sessions,
            reattach_pty,
            claim_popout_session,
            dock_popout_session,
            check_popout_main_overlap,
            focus_terminal_window,
            open_terminal_window,
            count_active_sessions,
            close_pty,
            close_all_ptys,
            get_agent_status,
            start_agent,
            add_key_to_agent,
            get_kerberos_status,
            renew_kerberos_ticket,
            acquire_kerberos_ticket,
            load_desktop_settings,
            save_desktop_settings,
            get_workspace_info,
            save_workspace_config,
            needs_onboarding,
            complete_onboarding,
            import_ssh_config,
            pick_ssh_config_file,
            quit_app,
            force_quit_app,
            commands::list_ssh_keys,
            commands::generate_ssh_key,
            commands::copy_ssh_key_to_target,
            commands::run_security_audit,
            commands::save_backup_file,
            commands::pick_backup_file,
            commands::export_connections_payload,
            commands::import_connections_payload,
            commands::fix_security_permissions,
            commands::ping_all_connections,
            commands::run_batch_command,
            commands::get_app_version,
            commands::list_remote_directory,
            tray::refresh_tray_menu,
            tray::send_desktop_notification,
            commands::get_env_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(unix)]
pub(crate) fn is_valid_socket(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixStream;
    let p = std::path::Path::new(path);
    if let Ok(meta) = p.metadata() {
        if meta.file_type().is_socket() {
            return UnixStream::connect(p).is_ok();
        }
    }
    false
}

#[cfg(not(unix))]
pub(crate) fn is_valid_socket(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

#[cfg(unix)]
fn init_shell_env() {
    // ── Strategy 1: read systemd user environment (covers PATH, DBUS, DISPLAY …) ──
    if let Ok(output) = std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
    {
        if output.status.success() {
            apply_env_vars(&String::from_utf8_lossy(&output.stdout));
        }
    }

    // ── Strategy 2: login-shell env (covers NVM, pyenv, .profile exports …) ──
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    if let Ok(output) = std::process::Command::new(&shell)
        .args(["-l", "-c", "env"])
        .output()
    {
        if output.status.success() {
            apply_env_vars(&String::from_utf8_lossy(&output.stdout));
        }
    }

    // ── Verify inherited SSH_AUTH_SOCK ──
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        if !is_valid_socket(&sock) {
            std::env::remove_var("SSH_AUTH_SOCK");
        }
    }

    // ── Strategy 3: probe for an SSH agent socket if none is set ──
    if std::env::var("SSH_AUTH_SOCK").is_err() {
        if let Some(sock) = find_ssh_agent_socket() {
            std::env::set_var("SSH_AUTH_SOCK", &sock);
            // Also propagate into the systemd user session for child processes
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "set-environment", &format!("SSH_AUTH_SOCK={sock}")])
                .output();
        }
    }
}

/// Walk the well-known places where SSH agent sockets are created and return
/// the first active, connectable SSH agent socket.
#[cfg(unix)]
pub(crate) fn find_ssh_agent_socket() -> Option<String> {
    // 1. Glob /tmp/ssh-*/agent.* (the classic openssh-agent pattern - highest priority)
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
            if !name.starts_with("ssh-") {
                continue;
            }
            if let Ok(files) = std::fs::read_dir(&dir) {
                for f in files.flatten() {
                    let fp = f.path();
                    let fname = fp.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if fname.starts_with("agent.") {
                        let path_str = fp.to_string_lossy().to_string();
                        if is_valid_socket(&path_str) {
                            return Some(path_str);
                        }
                    }
                }
            }
        }
    }

    // 2. Try common XDG_RUNTIME_DIR & desktop agent socket patterns
    let uid = unsafe { libc::getuid() };
    let runtime_dir = format!("/run/user/{uid}");
    let mut candidates = vec![
        format!("{runtime_dir}/ssh-agent.socket"),
        format!("{runtime_dir}/gcr/ssh"),
        format!("{runtime_dir}/keyring/ssh"),
        format!("{runtime_dir}/openssh_agent"),
        format!("{runtime_dir}/1password/t/agent.sock"),
    ];
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        candidates.push(format!("{home_str}/.1password/agent.sock"));
    }

    for c in &candidates {
        if is_valid_socket(c) {
            return Some(c.clone());
        }
    }

    // 3. Read /proc environ of running processes (excluding gpg-agent for standard key additions)
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let pid_path = entry.path();
            if !pid_path.is_dir() {
                continue;
            }
            let environ_path = pid_path.join("environ");
            if let Ok(data) = std::fs::read(&environ_path) {
                for kv in data.split(|&b| b == 0) {
                    let s = String::from_utf8_lossy(kv);
                    if let Some(val) = s.strip_prefix("SSH_AUTH_SOCK=") {
                        let sock = val.trim();
                        if !sock.is_empty() && !sock.contains("gnupg") && is_valid_socket(sock) {
                            return Some(sock.to_string());
                        }
                    }
                }
            }
        }
    }

    // 4. Fallback to GPG agent SSH socket bridge if no standard SSH agent is running
    let mut gpg_candidates = vec![
        format!("{runtime_dir}/gnupg/S.gpg-agent.ssh"),
    ];
    if let Some(home) = dirs::home_dir() {
        gpg_candidates.push(format!("{}/.gnupg/S.gpg-agent.ssh", home.to_string_lossy()));
    }
    for c in &gpg_candidates {
        if is_valid_socket(c) {
            return Some(c.clone());
        }
    }

    None
}


#[cfg(not(unix))]
pub(crate) fn find_ssh_agent_socket() -> Option<String> {
    std::env::var("SSH_AUTH_SOCK").ok().filter(|s| is_valid_socket(s))
}

/// Parse `KEY=VALUE` lines (from `env` or `systemctl show-environment`) and
/// selectively set the ones that SSH / Kerberos / PATH need.
#[cfg(unix)]
fn apply_env_vars(text: &str) {
    const KEYS: &[&str] = &[
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "KRB5CCNAME",
        "KRB5_CONFIG",
        "KRB5KEYTAB",
        "PATH",
        "USER",
        "LOGNAME",
        "HOME",
        "SHELL",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "DISPLAY",
        "WAYLAND_DISPLAY",
    ];
    for line in text.lines() {
        if let Some(pos) = line.find('=') {
            let key = &line[..pos];
            let val = &line[pos + 1..];
            if KEYS.contains(&key) {
                // Don't downgrade an already-good SSH_AUTH_SOCK from the
                // socket we found in Strategy 3.
                if key == "SSH_AUTH_SOCK" && std::env::var("SSH_AUTH_SOCK").as_deref().map(is_valid_socket).unwrap_or(false) {
                    continue;
                }
                std::env::set_var(key, val);
            }
        }
    }
}

