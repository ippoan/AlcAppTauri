use alc_config::{resolve_url_from, DEFAULT_APP_URL};

#[test]
fn tab_and_newline_are_trimmed() {
    assert_eq!(
        resolve_url_from(Some("\thttps://alc.ippoan.org\n")),
        "https://alc.ippoan.org"
    );
}

#[test]
fn empty_string_falls_back_to_default() {
    assert_eq!(resolve_url_from(Some("")), DEFAULT_APP_URL);
}

#[test]
fn none_falls_back_to_default() {
    assert_eq!(resolve_url_from(None), DEFAULT_APP_URL);
}

#[test]
fn only_whitespace_falls_back_to_default() {
    for w in ["   ", "\t", "\n", "  \t\n  "] {
        assert_eq!(resolve_url_from(Some(w)), DEFAULT_APP_URL, "input: {w:?}");
    }
}
