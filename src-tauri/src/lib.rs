use alc_config::AppConfig;

#[tauri::command]
fn get_app_config() -> AppConfig {
    alc_config::load()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_app_config])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
