use alc_config::AppConfig;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};
use tauri_plugin_updater::UpdaterExt;

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

/// 直近ログ snapshot をシステムクリップボードにコピーする。
/// arboard は Windows/macOS/Linux (X11/Wayland) に対応した薄いラッパ。
/// clipboard アクセス失敗は fail-open (warn ログのみ、キオスクを落とさない)。
fn copy_logs_to_clipboard(hub: &logws::LogHub) {
    let snapshot = hub.snapshot();
    let bytes = snapshot.len();
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(snapshot)) {
        Ok(_) => tracing::info!(bytes, "tray: logs copied to clipboard"),
        Err(e) => tracing::warn!(error = %e, "tray: clipboard set_text failed"),
    }
}

/// tray アイコン + メニューを構築する。メニュー: 画面を表示 / ログをコピー /
/// 再起動 / 終了。左クリックでも画面を復帰させる (無人キオスクでの操作性)。
///
/// `hub` を clone して "ログをコピー" ハンドラに move する (直近ログ snapshot を
/// クリップボードに書き込む)。WS が動かない環境での診断路。
fn build_tray(app: &tauri::App, hub: logws::LogHub) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "画面を表示", true, None::<&str>)?;
    let copy_logs = MenuItem::with_id(app, "copy_logs", "ログをコピー", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "再起動", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &copy_logs, &restart, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("AlcApp")
        .menu(&menu)
        // 左クリックはメニューを出さず window 復帰に使う (下の on_tray_icon_event)。
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "copy_logs" => copy_logs_to_clipboard(&hub),
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

/// 起動時と 1 時間ごとに update endpoint を polling し、新版があれば download +
/// install して再起動する。無人キオスク運用前提のため確認ダイアログは出さない
/// (installMode=passive)。ネットワーク失敗は fail-open (warn ログのみ、キオスク
/// を落とさない)。
async fn check_and_apply_updates(app: tauri::AppHandle) {
    loop {
        match app.updater() {
            Ok(updater) => match updater.check().await {
                Ok(Some(update)) => {
                    let ver = update.version.clone();
                    tracing::info!(new_version = %ver, "updater: new version available; downloading");
                    let mut total: u64 = 0;
                    match update
                        .download_and_install(
                            |chunk, _content_len| {
                                total += chunk as u64;
                            },
                            || tracing::info!("updater: download complete; installing"),
                        )
                        .await
                    {
                        Ok(_) => {
                            tracing::info!(new_version = %ver, downloaded = total, "updater: install complete; restarting");
                            app.restart();
                        }
                        Err(e) => tracing::warn!(error = %e, "updater: install failed"),
                    }
                }
                Ok(None) => tracing::debug!("updater: no update available"),
                Err(e) => tracing::warn!(error = %e, "updater: check failed"),
            },
            Err(e) => tracing::warn!(error = %e, "updater: not available"),
        }
        // 1 時間おきに再チェック。キオスクは長時間常駐する前提。
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ネイティブ層ログを WS ハブと stdout の両方へ。閲覧 UI は alc-app 側。
    let hub = logws::LogHub::new();
    logws::init_tracing(&hub);

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
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

            // 起動時 + 1h おきに updater check。pubkey が placeholder の間は
            // 実行時に endpoint 到達しても署名検証 or JSON parse で fail するが、
            // fail-open で warn するだけなのでキオスクは落ちない。
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                check_and_apply_updates(handle).await;
            });

            build_tray(app, hub.clone())?;
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
