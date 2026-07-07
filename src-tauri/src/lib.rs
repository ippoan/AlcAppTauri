use alc_config::AppConfig;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

mod logws;

#[tauri::command]
fn get_app_config() -> AppConfig {
    alc_config::load()
}

/// メインウィンドウを前面に出す (tray クリック / メニュー "show" 共通)。
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// tray アイコン + メニューを構築する。メニュー: 画面を表示 / 再起動 / 終了。
/// 左クリックでも画面を復帰させる (無人キオスクでの操作性)。
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "画面を表示", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "再起動", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &restart, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("AlcApp")
        .menu(&menu)
        // 左クリックはメニューを出さず window 復帰に使う (下の on_tray_icon_event)。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "restart" => app.restart(),
            "quit" => {
                tracing::info!("tray: quit selected");
                app.exit(0);
            }
            other => tracing::debug!(id = other, "tray: unknown menu id"),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    // bundle 済みアプリアイコンを流用 (専用 tray アイコンは持たない = 薄く保つ)。
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    } else {
        tracing::warn!("tray: no default window icon; tray shows without icon");
    }

    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ネイティブ層ログを WS ハブと stdout の両方へ。閲覧 UI は alc-app 側。
    let hub = logws::LogHub::new();
    logws::init_tracing(&hub);

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_app_config])
        .setup(move |app| {
            let cfg = alc_config::load();
            tracing::info!(url = %cfg.url, retry_ms = cfg.retry_interval_ms, "AlcApp starting");

            // dev ログ WS ハブ (127.0.0.1 のみ)。ALC_LOG_WS_PORT=0 で無効化。
            match cfg.log_ws_port {
                Some(port) => {
                    let hub = hub.clone();
                    tauri::async_runtime::spawn(async move {
                        logws::serve(hub, port).await;
                    });
                }
                None => tracing::info!("log ws: disabled (ALC_LOG_WS_PORT=0)"),
            }

            build_tray(app)?;
            Ok(())
        })
        // ウィンドウ "×" は終了せず tray へ最小化 (ブリッジ/ハブ常駐を継続)。
        // 明示終了は tray メニュー "終了" のみ。
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                    tracing::info!("main window close -> hidden to tray");
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
