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
    pick_ssh_config_file, quit_app, reattach_pty, remove_connection, remove_environment,
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
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(unix)]
fn init_shell_env() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    // Run a login shell and output its environment to override desktop session defaults
    if let Ok(output) = std::process::Command::new(&shell)
        .args(["-l", "-c", "env"])
        .output()
    {
        if output.status.success() {
            let env_str = String::from_utf8_lossy(&output.stdout);
            for line in env_str.lines() {
                if let Some(pos) = line.find('=') {
                    let key = &line[..pos];
                    let val = &line[pos + 1..];
                    if key == "SSH_AUTH_SOCK" || key == "KRB5CCNAME" || key == "PATH" {
                        std::env::set_var(key, val);
                    }
                }
            }
        }
    }
}
