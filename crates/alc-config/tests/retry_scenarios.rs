use alc_config::{resolve_retry_from, DEFAULT_RETRY_INTERVAL_MS};

#[test]
fn accepts_zero() {
    assert_eq!(resolve_retry_from(Some("0")), 0);
}

#[test]
fn large_value_is_parsed() {
    assert_eq!(resolve_retry_from(Some("60000")), 60000);
}

#[test]
fn negative_falls_back_to_default() {
    assert_eq!(resolve_retry_from(Some("-1")), DEFAULT_RETRY_INTERVAL_MS);
}

#[test]
fn whitespace_padded_number_is_parsed() {
    assert_eq!(resolve_retry_from(Some("  1500  ")), 1500);
}

#[test]
fn float_falls_back_to_default() {
    assert_eq!(resolve_retry_from(Some("2.5")), DEFAULT_RETRY_INTERVAL_MS);
}
