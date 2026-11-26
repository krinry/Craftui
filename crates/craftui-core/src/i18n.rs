//! Internationalization (i18n) translation catalogs, parameter interpolation, and RTL detection.

use crate::style::Direction;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Return `true` if `locale` (e.g. `"ar"`, `"ar-SA"`, `"he"`, `"fa"`, `"ur-PK"`) is right-to-left.
pub fn is_rtl_locale(locale: &str) -> bool {
    let base = locale
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        base.as_str(),
        "ar" | "he" | "iw" | "fa" | "ur" | "ps" | "sd" | "ug" | "yi" | "ckb" | "dv"
    )
}

/// Resolve the layout [`Direction`] corresponding to a BCP-47 locale tag.
pub fn locale_direction(locale: &str) -> Direction {
    if is_rtl_locale(locale) {
        Direction::Rtl
    } else {
        Direction::Ltr
    }
}

/// Portable JSON-backed string translation catalog with regional fallback and `{param}` substitution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizationCatalog {
    pub fallback_locale: String,
    pub tables: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for LocalizationCatalog {
    fn default() -> Self {
        Self::new("en")
    }
}

impl LocalizationCatalog {
    pub fn new(fallback_locale: impl Into<String>) -> Self {
        Self {
            fallback_locale: normalize_locale(&fallback_locale.into()),
            tables: BTreeMap::new(),
        }
    }

    /// Insert a single translation key/value pair for `locale`.
    pub fn insert(
        &mut self,
        locale: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) {
        let loc = normalize_locale(&locale.into());
        self.tables
            .entry(loc)
            .or_default()
            .insert(key.into(), value.into());
    }

    /// Load a flat JSON object (`{"key": "Translated text"}`) into `locale`.
    pub fn load_locale_json(
        &mut self,
        locale: impl Into<String>,
        json: &str,
    ) -> crate::Result<usize> {
        let map: BTreeMap<String, String> = serde_json::from_str(json)
            .map_err(|e| crate::CraftError::Platform(format!("invalid locale JSON: {e}")))?;
        let count = map.len();
        let loc = normalize_locale(&locale.into());
        self.tables.entry(loc).or_default().extend(map);
        Ok(count)
    }

    /// Look up `key` in `locale` (checking exact tag -> language subtag -> `fallback_locale`).
    /// Falls back to returning `key` itself if missing.
    pub fn translate(&self, locale: &str, key: &str) -> String {
        let norm = normalize_locale(locale);
        if let Some(val) = self.tables.get(&norm).and_then(|t| t.get(key)) {
            return val.clone();
        }
        if let Some((lang, _)) = norm.split_once('-')
            && let Some(val) = self.tables.get(lang).and_then(|t| t.get(key))
        {
            return val.clone();
        }
        if let Some(val) = self
            .tables
            .get(&self.fallback_locale)
            .and_then(|t| t.get(key))
        {
            return val.clone();
        }
        key.to_string()
    }

    /// Translate `key` for `locale` and substitute `{name}` placeholders using `params`.
    pub fn translate_with(&self, locale: &str, key: &str, params: &[(&str, &str)]) -> String {
        let mut out = self.translate(locale, key);
        for (name, val) in params {
            let token = format!("{{{name}}}");
            out = out.replace(&token, val);
        }
        out
    }

    /// All loaded locale codes.
    pub fn locales(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }
}

fn normalize_locale(locale: &str) -> String {
    locale.trim().replace('_', "-").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_rtl_detection_and_catalog_fallback_work() {
        assert_eq!(locale_direction("en-US"), Direction::Ltr);
        assert_eq!(locale_direction("ar-EG"), Direction::Rtl);
        assert_eq!(locale_direction("he"), Direction::Rtl);
        assert_eq!(locale_direction("ur_PK"), Direction::Rtl);

        let mut catalog = LocalizationCatalog::new("en");
        catalog
            .load_locale_json("en", r#"{"greeting": "Hello, {name}!", "save": "Save"}"#)
            .unwrap();
        catalog
            .load_locale_json("ar", r#"{"greeting": "مرحبا {name}"}"#)
            .unwrap();

        assert_eq!(
            catalog.translate_with("ar-SA", "greeting", &[("name", "Omar")]),
            "مرحبا Omar"
        );
        // Missing key in "ar" falls back cleanly to "en"
        assert_eq!(catalog.translate("ar-SA", "save"), "Save");
        // Unknown key returns key verbatim
        assert_eq!(catalog.translate("ar-SA", "missing_key"), "missing_key");
    }
}
