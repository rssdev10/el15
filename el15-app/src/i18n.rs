//! Tiny i18n layer. Locales live in `el15-app/locales/{en,ru,zh,es,hi}.json`.
//! Loaded at runtime via `rust-i18n`. Keys default to the English value.
//!
//! NB: the `rust_i18n::i18n!("locales", fallback = "en");` invocation lives
//! in `main.rs` (must be at the crate root for the macro to expand correctly).

pub use rust_i18n::t;

pub fn set_language(lang: &str) {
    rust_i18n::set_locale(lang);
}

pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("ru", "Русский"),
    ("zh", "中文"),
    ("es", "Español"),
    ("hi", "हिन्दी"),
];
