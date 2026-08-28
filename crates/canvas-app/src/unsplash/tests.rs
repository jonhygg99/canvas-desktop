//! Tests del cliente y los tipos de Unsplash: decodificado, errores sin
//! clave, mapping de filtros y parseo de la respuesta. Movidos del antiguo
//! `unsplash.rs` monolítico a la convención de un `tests.rs` por carpeta.

use super::*;
use crate::editor::EditorState;
use eframe::egui;
use types::{Photo, Urls, User};

/// Un PNG 2x2 en memoria (rojo, verde, azul, blanco) para `decode`.
fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_raw(
        2,
        2,
        vec![
            255, 0, 0, 255, //
            0, 255, 0, 255, //
            0, 0, 255, 255, //
            255, 255, 255, 255,
        ],
    )
    .unwrap();
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn decode_turns_png_bytes_into_loaded_image() {
    let img = decode(&tiny_png()).unwrap();
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(img.rgba.len(), 2 * 2 * 4);
    assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
}

#[test]
fn decode_rejects_garbage() {
    assert!(decode(b"not an image").is_err());
}

#[test]
fn search_without_key_is_an_error() {
    let had = std::env::var(ACCESS_KEY_ENV).ok();
    std::env::remove_var(ACCESS_KEY_ENV);
    let err = search("mountain", 1, SearchFilters::default()).unwrap_err();
    assert!(err.to_string().contains(ACCESS_KEY_ENV), "{err}");
    match had {
        Some(key) => std::env::set_var(ACCESS_KEY_ENV, key),
        None => std::env::remove_var(ACCESS_KEY_ENV),
    }
}

#[test]
fn panel_defaults_to_empty() {
    let p = Panel::default();
    assert!(p.query.is_empty());
    assert!(p.photos.is_empty());
    assert!(p.error.is_none());
    assert!(!p.searching);
    assert!(!p.reached_end);
    assert_eq!(p.filters, SearchFilters::default());
    assert_eq!(p.search_seq, 0);
}

#[test]
fn orientation_maps_to_api_values() {
    assert_eq!(Orientation::Any.as_str(), None);
    assert_eq!(Orientation::Landscape.as_str(), Some("landscape"));
    assert_eq!(Orientation::Portrait.as_str(), Some("portrait"));
    assert_eq!(Orientation::Squarish.as_str(), Some("squarish"));
}

#[test]
fn color_maps_to_api_values() {
    assert_eq!(ColorFilter::Any.as_str(), None);
    assert_eq!(ColorFilter::BlackAndWhite.as_str(), Some("black_and_white"));
    assert_eq!(ColorFilter::Red.as_str(), Some("red"));
    assert_eq!(ColorFilter::Teal.as_str(), Some("teal"));
    // Todos los colores tienen etiqueta y punto de UI (excepto «Any»).
    for c in ColorFilter::ALL {
        assert!(!c.label().is_empty());
        if c == ColorFilter::Any {
            assert!(c.swatch().is_none());
        } else {
            assert!(c.swatch().is_some(), "{} sin swatch", c.label());
        }
    }
}

#[test]
fn order_by_maps_to_api_values() {
    assert_eq!(OrderBy::Relevant.as_str(), "relevant");
    assert_eq!(OrderBy::Latest.as_str(), "latest");
    assert_eq!(OrderBy::default(), OrderBy::Relevant);
}

#[test]
fn filters_default_to_no_restrictions() {
    let f = SearchFilters::default();
    assert_eq!(f.orientation, Orientation::Any);
    assert_eq!(f.color, ColorFilter::Any);
    assert_eq!(f.order_by, OrderBy::Relevant);
}

#[test]
fn reached_end_is_true_when_total_pages_says_so() {
    assert!(reached_end(Some(1), 1, PER_PAGE as usize));
    assert!(reached_end(Some(3), 3, PER_PAGE as usize));
    assert!(!reached_end(Some(3), 2, PER_PAGE as usize));
}

#[test]
fn reached_end_falls_back_to_short_page() {
    // Sin `total_pages` en la respuesta, una página incompleta es el fin.
    assert!(reached_end(None, 1, 7));
    // Una página completa sin `total_pages` no es necesariamente el fin.
    assert!(!reached_end(None, 1, PER_PAGE as usize));
}

#[test]
fn search_response_parses_total_pages() {
    let json = r#"{"total_pages": 4, "results": []}"#;
    let parsed: SearchResponse = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.total_pages, Some(4));
    // El campo puede faltar (respuestas antiguas): se ignora.
    let parsed: SearchResponse = serde_json::from_str(r#"{"results": []}"#).unwrap();
    assert_eq!(parsed.total_pages, None);
}

// ---- Humo de render headless del panel y sus tarjetas ----

/// El panel Unsplash se pinta sin pánico en su estado inicial. Sin clave
/// en el entorno (los tests no definen `UNSPLASH_ACCESS_KEY`) muestra la
/// guía; con clave, la barra de búsqueda: en ambos casos pinta algo.
#[test]
fn panel_ui_renders_without_panic_and_paints() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut state = EditorState::new_blank(800.0, 600.0);
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(360.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            panel_ui(&mut state, ui, &tx);
        },
    );
    assert!(
        !out.shapes.is_empty(),
        "el panel debe pintar su estado inicial"
    );
}

/// Las tarjetas de resultado se pintan sin miniatura (el fondo de la
/// tarjeta actúa de placeholder) y sin pánico — humo del camino de render
/// del grid de resultados con `thumb: None`.
#[test]
fn photo_cards_render_without_a_thumbnail_and_without_panicking() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut items: Vec<PhotoItem> = (0..2)
        .map(|i| PhotoItem {
            photo: Photo {
                id: format!("p{i}"),
                urls: Urls {
                    small: format!("https://example.com/{i}/small"),
                    regular: format!("https://example.com/{i}/regular"),
                },
                user: User {
                    name: format!("Author {i}"),
                },
            },
            thumb: None,
            thumb_failed: false,
        })
        .collect();
    let mut inserting = None;
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(360.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            for item in &mut items {
                card::photo_card_ui(item, &mut inserting, 300.0, 200.0, ui, &tx);
                ui.add_space(12.0);
            }
        },
    );
    assert!(
        out.shapes.len() >= 2,
        "cada tarjeta debe pintar al menos su fondo de placeholder"
    );
}
