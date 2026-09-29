//! Tests del worker masivo web: tallo de archivo y nombre libre sin
//! colisiones. Sin red ni GPU: puro sistema de archivos temporal.

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
fn free_bulk_path_skips_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    // `shakira-01.png` ya existe: la primera libre es la 02.
    File::create(dir.path().join("shakira-01.png")).unwrap();
    assert_eq!(
        free_bulk_path(dir.path(), "shakira", 0),
        dir.path().join("shakira-02.png")
    );
    // El índice cuenta desde ahí: con índice 5 pide la 06.
    assert_eq!(
        free_bulk_path(dir.path(), "shakira", 5),
        dir.path().join("shakira-06.png")
    );
}

#[test]
fn common_page_size_is_max_dims_or_fallback() {
    let item = |w, h| BulkItem {
        url: "https://a.com/1.jpg".to_owned(),
        label: "Web".to_owned(),
        width: w,
        height: h,
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
fn resolve_bulk_page_honors_the_setting() {
    use crate::settings::BulkCanvasSize;
    let item = |w, h| BulkItem {
        url: "https://a.com/1.jpg".to_owned(),
        label: "Web".to_owned(),
        width: w,
        height: h,
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
