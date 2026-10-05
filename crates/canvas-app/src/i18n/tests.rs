use super::*;

#[test]
fn switching_language_changes_ui_but_preserves_unknown_names() {
    set_language(Language::Spanish);
    assert_eq!(tr("Settings"), "Ajustes");
    assert_eq!(tr("family-photo.png"), "family-photo.png");
    set_language(Language::English);
    assert_eq!(tr("Settings"), "Settings");
}

#[test]
fn language_is_persisted_and_old_settings_default_to_english() {
    let mut settings: crate::settings::AppSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.language, Language::English);
    settings.language = Language::Spanish;
    let restored: crate::settings::AppSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored.language, Language::Spanish);
}
