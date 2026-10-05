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

#[test]
fn spanish_settings_render_translated_controls_with_unchanged_enum_values() {
    let ctx = eframe::egui::Context::default();
    let mut settings = crate::settings::AppSettings {
        language: Language::Spanish,
        ..Default::default()
    };
    set_language(settings.language);
    let mut open = true;
    for frame in 0..3 {
        let output = ctx.run_ui(Default::default(), |_| {
            crate::settings::settings_window(&ctx, &mut settings, &mut open, "");
        });
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                eframe::egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        if frame == 2 {
            assert!(text.contains("Idioma"), "rendered settings: {text}");
            assert!(text.contains("Densidad de interfaz"));
            assert!(text.contains("Sistema"));
        }
    }
    assert_eq!(settings.language, Language::Spanish);
    set_language(Language::English);
}
