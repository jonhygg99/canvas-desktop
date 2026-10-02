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
fn bulk_canvas_size_defaults_to_full_hd() {
    assert_eq!(BulkCanvasSize::default(), BulkCanvasSize::FullHd1920);
    assert_eq!(BulkCanvasSize::FullHd1920.label(), "1920 × 1080");
    assert_eq!(BulkCanvasSize::FullHd1920.dims(), Some((1920.0, 1080.0)));
    assert_eq!(BulkCanvasSize::Hd1280.dims(), Some((1280.0, 720.0)));
    assert_eq!(BulkCanvasSize::Square1080.dims(), Some((1080.0, 1080.0)));
    assert_eq!(BulkCanvasSize::BatchMax.dims(), None);
    assert_eq!(
        AppSettings::default().serper_bulk_size,
        BulkCanvasSize::FullHd1920
    );
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

// ——— Escritor serial (A10): un snapshot viejo nunca sobrescribe a uno nuevo ———

use super::writer::{run_writer, write_snapshot};

fn settings_with_quality_jpeg(quality: u8) -> AppSettings {
    AppSettings {
        jpeg_quality: quality,
        ..AppSettings::default()
    }
}

#[test]
fn write_snapshot_round_trips_through_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sub").join("settings.json");
    let before = settings_with_quality_jpeg(70);
    write_snapshot(&path, &before);
    let text = std::fs::read_to_string(&path).expect("escrito");
    let back: AppSettings = serde_json::from_str(&text).expect("parseable");
    assert!(back == before, "el disco debe guardar el snapshot tal cual");
}

#[test]
fn serial_writer_keeps_only_the_newest_revision() {
    // A10: dos snapshots encolados seguidos (el viejo aún sin escribir
    // cuando llega el nuevo) dejan en disco el NUEVO, nunca el viejo.
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc,
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");
    let (tx, rx) = mpsc::channel();
    let done = Arc::new(AtomicU64::new(0));
    std::thread::spawn({
        let done = Arc::clone(&done);
        let path = Some(path.clone());
        move || run_writer(rx, done, path)
    });
    tx.send((1, settings_with_quality_jpeg(11)))
        .expect("enviar rev 1");
    tx.send((2, settings_with_quality_jpeg(77)))
        .expect("enviar rev 2");
    drop(tx);
    let start = std::time::Instant::now();
    while done.load(Ordering::SeqCst) < 2 {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "el escritor no drenó la cola"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let text = std::fs::read_to_string(&path).expect("escrito");
    let back: AppSettings = serde_json::from_str(&text).expect("parseable");
    assert_eq!(
        back.jpeg_quality, 77,
        "en disco debe quedar la revisión nueva, no la vieja"
    );
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
