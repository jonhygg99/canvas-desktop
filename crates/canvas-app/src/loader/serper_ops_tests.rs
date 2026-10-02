//! Tests del worker masivo web: tallo de archivo y reserva atómica de
//! nombres sin colisiones. Sin red ni GPU: puro sistema de archivos temporal.

use super::*;
use std::fs::File;

#[test]
fn slugify_is_a_safe_filename_stem() {
    assert_eq!(slugify("Gato Montés 2026!"), "gato-mont-s-2026");
    assert_eq!(slugify("   "), "web");
    assert_eq!(slugify("a"), "a");
    let long = "x".repeat(100);
    assert!(slugify(&long).len() <= 30);
}

#[test]
fn bulk_reservation_skips_existing_files_and_holds_the_name() {
    // A06: la reserva es atómica (`create_new`), no un `exists()` suelto.
    let dir = tempfile::tempdir().unwrap();
    // `shakira-01.png` ya existe: la primera libre es la 02, y reservarla la
    // crea en disco para que otra tanda no la obtenga.
    File::create(dir.path().join("shakira-01.png")).unwrap();
    let p = canvas_io::reserve_bulk_path(dir.path(), "shakira", "png", 1).unwrap();
    assert_eq!(p, dir.path().join("shakira-02.png"));
    assert!(p.is_file(), "la reserva crea el hueco en disco");
    // La siguiente reserva ya no puede obtener la 02: gana la 03.
    let q = canvas_io::reserve_bulk_path(dir.path(), "shakira", "png", 1).unwrap();
    assert_eq!(q, dir.path().join("shakira-03.png"));
    // El índice cuenta desde ahí: con índice 5 pide la 06.
    let r = canvas_io::reserve_bulk_path(dir.path(), "shakira", "png", 6).unwrap();
    assert_eq!(r, dir.path().join("shakira-06.png"));
}

#[test]
fn common_page_size_is_max_dims_or_fallback() {
    let item = |w, h| BulkItem {
        url: "https://a.com/1.jpg".to_owned(),
        label: "Web".to_owned(),
        width: w,
        height: h,
        post_url: "https://a.com/p".to_owned(),
        thumb_url: None,
    };
    // Máx. ancho × máx. alto aunque vengan de fotos distintas.
    assert_eq!(
        common_page_size(&[item(Some(800), Some(600)), item(Some(400), Some(1200))]),
        (800.0, 1200.0)
    );
    // Sin dims conocidas: 1920×1080.
    assert_eq!(common_page_size(&[item(None, None)]), (1920.0, 1080.0));
    assert_eq!(common_page_size(&[]), (1920.0, 1080.0));
    // Dims a cero no cuentan.
    assert_eq!(
        common_page_size(&[item(Some(0), Some(0)), item(Some(640), Some(480))]),
        (640.0, 480.0)
    );
}

#[test]
fn default_bulk_dir_uses_env_override_and_slug() {
    std::env::set_var("SERPER_BULK_DIR", "C:/tmp/bulk-test");
    assert_eq!(
        default_bulk_dir("Shakira!"),
        Some(std::path::PathBuf::from("C:/tmp/bulk-test"))
    );
    std::env::remove_var("SERPER_BULK_DIR");
}

#[test]
fn bulk_creates_missing_folder() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("missing").join("deep");
    assert!(!folder.exists());
    // Tanda vacía: solo debe crear la carpeta y terminar con 0 creados.
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = eframe::egui::Context::default();
    spawn_serper_bulk_files(
        Vec::new(),
        folder.clone(),
        "x".to_owned(),
        (10.0, 10.0),
        tx,
        ctx,
    );
    let msg = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(matches!(msg, crate::loader::AppMsg::SerperBulkDone { .. }));
    if let crate::loader::AppMsg::SerperBulkDone {
        created, errors, ..
    } = msg
    {
        assert!(created.is_empty());
        assert!(errors.is_empty());
    }
    assert!(folder.exists(), "la carpeta debe crearse sola");
}

#[test]
fn resolve_bulk_page_honors_the_setting() {
    use crate::settings::BulkCanvasSize;
    let item = |w, h| BulkItem {
        url: "https://a.com/1.jpg".to_owned(),
        label: "Web".to_owned(),
        width: w,
        height: h,
        post_url: "https://a.com/p".to_owned(),
        thumb_url: None,
    };
    let items = vec![item(Some(800), Some(600))];
    // Por defecto: Full HD aunque la tanda sea pequeña.
    assert_eq!(
        resolve_bulk_page(BulkCanvasSize::default(), &items),
        (1920.0, 1080.0)
    );
    assert_eq!(
        resolve_bulk_page(BulkCanvasSize::Hd1280, &items),
        (1280.0, 720.0)
    );
    // BatchMax sí mira la tanda.
    assert_eq!(
        resolve_bulk_page(BulkCanvasSize::BatchMax, &items),
        (800.0, 600.0)
    );
}

#[test]
fn fit_thumb_dims_keeps_aspect_within_the_cap() {
    // A11: miniaturas para GPU, nunca la foto completa.
    assert_eq!(fit_thumb_dims(800, 600), (512, 384));
    assert_eq!(fit_thumb_dims(600, 800), (384, 512));
    assert_eq!(fit_thumb_dims(512, 512), (512, 512));
    assert_eq!(fit_thumb_dims(100, 80), (100, 80), "pequeña: intacta");
    assert_eq!(fit_thumb_dims(0, 0), (1, 1), "degenerada: sin ceros");
}

#[test]
fn clamp_bulk_page_rejects_huge_api_dims_without_allocating() {
    // A11: 20000×20000 (≈1,5 GiB de RGBA) no llega al horneado; el rechazo
    // es aritmética pura, sin materializar la imagen.
    let (w, h) = clamp_bulk_page(20_000, 20_000);
    assert!(
        w <= MAX_BULK_PAGE_LONG && h <= MAX_BULK_PAGE_LONG,
        "{w}x{h}"
    );
    assert!(
        u64::from(w) * u64::from(h) <= MAX_BULK_PAGE_PIXELS,
        "{w}x{h}"
    );
    assert_eq!((w, h), (4096, 4096));
    // Apaisada extrema: manda el lado mayor, con aspecto.
    assert_eq!(clamp_bulk_page(20_000, 100), (4096, 20));
    // Sanas: intactas. Límite exacto: intacto.
    assert_eq!(clamp_bulk_page(1920, 1080), (1920, 1080));
    assert_eq!(clamp_bulk_page(4096, 4096), (4096, 4096));
    assert_eq!(clamp_bulk_page(0, 0), (1, 1));
}
