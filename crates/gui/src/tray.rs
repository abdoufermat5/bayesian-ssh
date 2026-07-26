use std::process::Command;

use tauri::{
    include_image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

// Tray icons must stay small; use the dedicated 32x32 asset instead of the 512px window icon.
const TRAY_ICON: tauri::image::Image<'static> = include_image!("icons/32x32.png");

use crate::commands::{close_all_ptys, get_db_and_config, PtyState};

pub fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let show_i = MenuItemBuilder::new("Open Bayesian SSH").id("show").build(app)?;
    let quit_i = MenuItemBuilder::new("Quit").id("quit").build(app)?;

    let mut submenu_builder = SubmenuBuilder::new(app, "Quick Connect");
    let mut has_connections = false;

    if let Ok((db, _config)) = get_db_and_config() {
        if let Ok(conns) = db.list_connections(None, false) {
            for conn in conns.iter().take(10) {
                let id = format!("connect:{}", conn.name);
                if let Ok(item) = MenuItemBuilder::new(&conn.name).id(id).build(app) {
                    submenu_builder = submenu_builder.item(&item);
                    has_connections = true;
                }
            }
        }
    }

    if !has_connections {
        if let Ok(item) = MenuItemBuilder::new("No Servers Configured")
            .id("no_servers")
            .enabled(false)
            .build(app)
        {
            submenu_builder = submenu_builder.item(&item);
        }
    }

    let submenu = submenu_builder.build()?;

    let menu = MenuBuilder::new(app)
        .item(&show_i)
        .separator()
        .item(&submenu)
        .separator()
        .item(&quit_i)
        .build()?;

    Ok(menu)
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_tray_menu(app)?;

    let _tray = TrayIconBuilder::new()
        .tooltip("Bayesian SSH")
        .icon(TRAY_ICON.clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            if id == "show" {
                show_main_window(app);
            } else if id == "quit" {
                quit_application(app);
            } else if let Some(host_name) = id.strip_prefix("connect:") {
                show_main_window(app);
                let _ = app.emit::<String>("connect-host", host_name.to_string());
            }
        })
        .on_tray_icon_event(|tray: &tauri::tray::TrayIcon<tauri::Wry>, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn quit_application(app: &AppHandle) {
    if let Some(state) = app.try_state::<PtyState>() {
        let _ = close_all_ptys(app.clone(), state);
    }
    app.exit(0);
}

#[tauri::command]
pub fn refresh_tray_menu(app: AppHandle) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id("default") {
        let menu = build_tray_menu(&app).map_err(|e| e.to_string())?;
        let _ = tray.set_menu(Some(menu));
    }
    Ok(())
}

#[tauri::command]
pub fn send_desktop_notification(title: String, body: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("notify-send")
            .arg("-a")
            .arg("Bayesian SSH")
            .arg(&title)
            .arg(&body)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!("display notification \"{}\" with title \"{}\"", body, title);
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "[void] [System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms'); \
             $objNotifyIcon = New-Object System.Windows.Forms.NotifyIcon; \
             $objNotifyIcon.Icon = [System.Drawing.SystemIcons]::Information; \
             $objNotifyIcon.BalloonTipText = '{}'; \
             $objNotifyIcon.BalloonTipTitle = '{}'; \
             $objNotifyIcon.Visible = $True; \
             $objNotifyIcon.ShowBalloonTip(5000)",
            body, title
        );
        let _ = Command::new("powershell")
            .arg("-Command")
            .arg(&script)
            .spawn();
    }
    Ok(())
}
