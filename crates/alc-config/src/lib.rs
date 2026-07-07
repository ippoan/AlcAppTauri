use serde::Serialize;

pub const DEFAULT_APP_URL: &str = "https://alc.ippoan.org";
pub const DEFAULT_RETRY_INTERVAL_MS: u64 = 5000;
/// ネイティブ層 (Rust) ログを配信する 127.0.0.1 WS ハブの既定ポート。
/// 既存ブリッジ (NFC 9876 / BLE 9877 / FC-1200 9878) と衝突しない値を選ぶ。
/// 閲覧 UI は alc-app 側 (dev ログビューア) が `ws://127.0.0.1:<port>` に繋ぐ。
pub const DEFAULT_LOG_WS_PORT: u16 = 9880;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppConfig {
    pub url: String,
    #[serde(rename = "retryIntervalMs")]
    pub retry_interval_ms: u64,
    /// ネイティブ層ログ WS ハブの listen ポート。`None` = 無効 (`ALC_LOG_WS_PORT=0`)。
    /// alc-app の dev ログビューアはこの値で接続先を決められる。
    #[serde(rename = "logWsPort", skip_serializing_if = "Option::is_none")]
    pub log_ws_port: Option<u16>,
}

pub fn resolve_url_from(raw: Option<&str>) -> String {
    match raw {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => DEFAULT_APP_URL.to_string(),
    }
}

pub fn resolve_retry_from(raw: Option<&str>) -> u64 {
    match raw.map(str::trim) {
        Some(s) if !s.is_empty() => s.parse::<u64>().unwrap_or(DEFAULT_RETRY_INTERVAL_MS),
        _ => DEFAULT_RETRY_INTERVAL_MS,
    }
}

/// `ALC_LOG_WS_PORT` の解決。未設定/空 → 既定 9880。`"0"` → `None` (明示無効化)。
/// 不正値 → 既定にフォールバック (無音失敗を避けつつ起動を止めない)。
pub fn resolve_log_ws_port_from(raw: Option<&str>) -> Option<u16> {
    match raw.map(str::trim) {
        None | Some("") => Some(DEFAULT_LOG_WS_PORT),
        Some("0") => None,
        Some(s) => Some(s.parse::<u16>().unwrap_or(DEFAULT_LOG_WS_PORT)),
    }
}

pub fn load() -> AppConfig {
    let url = resolve_url_from(std::env::var("ALC_APP_URL").ok().as_deref());
    let retry_interval_ms =
        resolve_retry_from(std::env::var("ALC_RETRY_INTERVAL_MS").ok().as_deref());
    let log_ws_port = resolve_log_ws_port_from(std::env::var("ALC_LOG_WS_PORT").ok().as_deref());
    AppConfig {
        url,
        retry_interval_ms,
        log_ws_port,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_defaults_when_unset() {
        assert_eq!(resolve_url_from(None), DEFAULT_APP_URL);
    }
    #[test]
    fn url_defaults_when_empty_or_whitespace() {
        assert_eq!(resolve_url_from(Some("")), DEFAULT_APP_URL);
        assert_eq!(resolve_url_from(Some("   ")), DEFAULT_APP_URL);
    }
    #[test]
    fn url_uses_override_trimmed() {
        assert_eq!(
            resolve_url_from(Some("  https://alc-staging.ippoan.org  ")),
            "https://alc-staging.ippoan.org"
        );
    }
    #[test]
    fn retry_defaults_when_unset_or_empty() {
        assert_eq!(resolve_retry_from(None), DEFAULT_RETRY_INTERVAL_MS);
        assert_eq!(resolve_retry_from(Some("")), DEFAULT_RETRY_INTERVAL_MS);
    }
    #[test]
    fn retry_defaults_on_invalid() {
        assert_eq!(resolve_retry_from(Some("abc")), DEFAULT_RETRY_INTERVAL_MS);
    }
    #[test]
    fn retry_parses_valid() {
        assert_eq!(resolve_retry_from(Some("2500")), 2500);
    }
    #[test]
    fn log_ws_port_defaults_when_unset_or_empty() {
        assert_eq!(resolve_log_ws_port_from(None), Some(DEFAULT_LOG_WS_PORT));
        assert_eq!(
            resolve_log_ws_port_from(Some("")),
            Some(DEFAULT_LOG_WS_PORT)
        );
        assert_eq!(
            resolve_log_ws_port_from(Some("  ")),
            Some(DEFAULT_LOG_WS_PORT)
        );
    }
    #[test]
    fn log_ws_port_zero_disables() {
        assert_eq!(resolve_log_ws_port_from(Some("0")), None);
        assert_eq!(resolve_log_ws_port_from(Some(" 0 ")), None);
    }
    #[test]
    fn log_ws_port_parses_valid() {
        assert_eq!(resolve_log_ws_port_from(Some("9999")), Some(9999));
        assert_eq!(resolve_log_ws_port_from(Some(" 12345 ")), Some(12345));
    }
    #[test]
    fn log_ws_port_defaults_on_invalid() {
        assert_eq!(
            resolve_log_ws_port_from(Some("nope")),
            Some(DEFAULT_LOG_WS_PORT)
        );
        // u16 範囲外もフォールバック
        assert_eq!(
            resolve_log_ws_port_from(Some("70000")),
            Some(DEFAULT_LOG_WS_PORT)
        );
    }
}
