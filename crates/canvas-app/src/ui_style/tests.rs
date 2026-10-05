use super::*;

#[test]
fn invalid_scale_cannot_hide_or_break_the_interface() {
    assert_eq!(normalized_scale(f32::NAN), 1.0);
    assert_eq!(normalized_scale(f32::INFINITY), 1.0);
    assert_eq!(normalized_scale(0.1), 0.75);
    assert_eq!(normalized_scale(5.0), 2.0);
}

#[test]
fn old_preferences_gain_accessible_defaults_and_roundtrip() {
    let mut settings: AppSettings = serde_json::from_str(r#"{"jpeg_quality":80}"#).unwrap();
    assert_eq!(settings.density, Density::Comfortable);
    assert_eq!(settings.ui_scale, 1.0);
    settings.density = Density::Compact;
    settings.ui_scale = 1.5;
    settings.reduced_motion = true;
    let restored: AppSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert!(restored == settings);
}
