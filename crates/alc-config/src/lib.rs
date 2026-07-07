use serde::Serialize;

pub const DEFAULT_APP_URL: &str = "https://alc.ippoan.org";
pub const DEFAULT_RETRY_INTERVAL_MS: u64 = 5000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppConfig {
    pub url: String,
    #[serde(rename = "retryIntervalMs")]
    pub retry_interval_ms: u64,
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

pub fn load() -> AppConfig {
    let url = resolve_url_from(std::env::var("ALC_APP_URL").ok().as_deref());
    let retry_interval_ms =
        resolve_retry_from(std::env::var("ALC_RETRY_INTERVAL_MS").ok().as_deref());
    AppConfig {
        url,
        retry_interval_ms,
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
}
