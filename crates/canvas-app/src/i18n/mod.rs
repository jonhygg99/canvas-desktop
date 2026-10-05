//! Traducciones de interfaz. Los nombres y el contenido del documento no se traducen.
use serde::{Deserialize, Serialize};
use std::{cell::Cell, collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    English,
    Spanish,
}

thread_local! { static LANGUAGE: Cell<Language> = const { Cell::new(Language::English) }; }

pub fn set_language(language: Language) {
    LANGUAGE.set(language);
}

pub fn tr(text: &str) -> &str {
    if LANGUAGE.get() == Language::English {
        return text;
    }
    static CATALOG: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("es.json")).expect("valid translation catalog")
        })
        .get(text)
        .map_or(text, String::as_str)
}

pub fn command() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    }
}

#[cfg(test)]
mod tests;
