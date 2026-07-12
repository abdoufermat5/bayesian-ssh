mod commands;

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use commands::{PtyState, get_active_env, set_active_env, list_environments, create_environment, remove_environment, get_connections, add_connection, edit_connection, remove_connection, get_stats, get_history, pick_key_file, spawn_pty, write_pty, resize_pty, close_pty, get_agent_status, start_agent, add_key_to_agent, load_desktop_settings, save_desktop_settings};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(PtyState {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_background_color(Some(tauri::window::Color(9, 9, 11, 255)));
            }
            Ok(())
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
            close_pty,
            get_agent_status,
            start_agent,
            add_key_to_agent,
            load_desktop_settings,
            save_desktop_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

