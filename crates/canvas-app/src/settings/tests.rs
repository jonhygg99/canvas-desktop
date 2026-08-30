//! Tests de los ajustes: `natural_cmp`, helpers de enums y round-trip
//! JSON de `AppSettings`.

use super::*;

#[test]
fn natural_cmp_orders_numbered_filenames_numerically() {
    let mut names = vec!["6.png", "51.png", "10.png", "1.png", "9.png"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(names, vec!["1.png", "6.png", "9.png", "10.png", "51.png"]);
}

#[test]
fn natural_cmp_is_case_insensitive_on_the_non_numeric_parts() {
    assert_eq!(
        natural_cmp("Photo2.png", "photo10.png"),
        std::cmp::Ordering::Less
    );
}

#[test]
fn natural_cmp_treats_leading_zeros_as_the_same_number() {
    assert_eq!(natural_cmp("007.png", "7.png"), std::cmp::Ordering::Equal);
}

#[test]
fn natural_cmp_falls_back_to_plain_text_without_digits() {
    let mut names = vec!["banana.png", "apple.png", "cherry.png"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(names, vec!["apple.png", "banana.png", "cherry.png"]);
}

#[test]
fn layers_tab_order_swapped_round_trips() {
    assert_eq!(
        LayersTabOrder::PageFirst.swapped(),
        LayersTabOrder::LayersFirst
    );
    assert_eq!(
        LayersTabOrder::LayersFirst.swapped(),
        LayersTabOrder::PageFirst
    );
}

#[test]
fn enum_helpers_expose_labels_and_extensions() {
    assert_eq!(ThemeChoice::System.label(), "System");
    assert_eq!(ThemeChoice::Light.label(), "Light");
    assert_eq!(ThemeChoice::Dark.label(), "Dark");
    assert_eq!(NewCanvasFormat::Png.label(), "PNG image");
    assert_eq!(NewCanvasFormat::Jpeg.extension(), "jpg");
    assert_eq!(NewCanvasFormat::WebP.extension(), "webp");
    assert_eq!(
        NewCanvasFormat::Canvas.extension(),
        canvas_io::CANVAS_EXTENSION
    );
    assert_eq!(GallerySort::Name.label(), "Name");
    assert_eq!(GallerySort::DateModified.label(), "Date modified");
    assert_eq!(GallerySort::Manual.label(), "Manual order");
    assert_eq!(StripSide::Top.label(), "Top");
}

#[test]
fn app_settings_round_trip_through_json() {
    let s = AppSettings {
        theme: ThemeChoice::Dark,
        new_canvas_format: NewCanvasFormat::WebP,
        layers_tab_order: LayersTabOrder::LayersFirst,
        jpeg_quality: 70,
        gallery_sort: GallerySort::DateModified,
        recent_files: vec![PathBuf::from("/a.png"), PathBuf::from("/b.png")],
        last_page_size: (800.0, 600.0),
        ..AppSettings::default()
    };

    let json = serde_json::to_string(&s).unwrap();
    let back: AppSettings = serde_json::from_str(&json).unwrap();
    assert!(back == s, "un viaje de ida y vuelta no debe perder nada");
}

#[test]
fn new_canvas_default_is_full_hd() {
    assert_eq!(AppSettings::default().last_page_size, (1920.0, 1080.0));
}

#[test]
fn a_partial_settings_json_fills_the_missing_fields_with_defaults() {
    // `#[serde(default)]`: un JSON antiguo o incompleto no debe romper
    // la carga — los campos que faltan toman los valores por defecto.
    let json = r#"{"jpeg_quality": 60}"#;
    let s: AppSettings = serde_json::from_str(json).unwrap();
    assert_eq!(s.jpeg_quality, 60);
    assert!(s.theme == ThemeChoice::default());
    assert!(s.new_canvas_format == NewCanvasFormat::default());
    assert!(s.layers_tab_order == LayersTabOrder::default());
    assert!(s.recent_files.is_empty());
    assert_eq!(s.last_page_size, (1920.0, 1080.0));
}
