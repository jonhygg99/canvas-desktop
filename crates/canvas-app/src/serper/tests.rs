//! Tests del módulo Web/Serper: presupuesto de tokens, query con tope de
//! palabras, clasificación del filtro, parseo de la respuesta y humo de
//! render headless del panel y sus tarjetas.

use super::*;
use crate::editor::EditorState;
use crate::settings::AppSettings;
use eframe::egui;
use filter::{
    apply_filter, build_query, extension_allowed, host_blocked, host_of, ratio_blocked,
    title_blocked, url_pattern_blocked, DropReason, MAX_QUERY_WORDS,
};
use types::SerperPhoto;

fn photo(url: &str, title: &str, w: Option<u32>, h: Option<u32>) -> SerperPhoto {
    SerperPhoto {
        id: url.to_owned(),
        title: title.to_owned(),
        image_url: url.to_owned(),
        width: w,
        height: h,
        source_url: "https://example.com/page".to_owned(),
    }
}

fn no_block() -> Vec<String> {
    Vec::new()
}

// ---- Presupuesto de tokens ----

#[test]
fn token_budget_maps_to_calls_images_and_credits() {
    // 1 token ≈ 10 imgs/1cr, 2 tokens ≈ 100 imgs/2cr, 3 tokens ≈ 200/4cr.
    assert_eq!(TokenBudget::One.calls(), 1);
    assert_eq!(TokenBudget::Two.calls(), 1);
    assert_eq!(TokenBudget::Three.calls(), 2);
    assert_eq!(TokenBudget::One.num(), 10);
    assert_eq!(TokenBudget::Two.num(), 100);
    assert_eq!(TokenBudget::One.max_images(), 10);
    assert_eq!(TokenBudget::Two.max_images(), 100);
    assert_eq!(TokenBudget::Three.max_images(), 200);
    assert_eq!(TokenBudget::One.credits(), 1);
    assert_eq!(TokenBudget::Two.credits(), 2);
    assert_eq!(TokenBudget::Three.credits(), 4);
    assert_eq!(TokenBudget::default(), TokenBudget::One);
}

#[test]
fn panel_spending_is_capped_by_the_budget() {
    let mut p = Panel::default();
    assert!(p.can_spend_more());
    // Buscar una vez desactiva hasta que llegue la respuesta.
    p.searching = true;
    assert!(!p.can_spend_more());
    p.searching = false;
    // Con presupuesto 1 y un token gastado no hay más gasto posible.
    p.tokens_spent = 1;
    assert!(!p.can_spend_more());
    // Con 3 tokens caben 2 llamadas: la segunda aún pasa, la tercera no.
    p.budget = TokenBudget::Three;
    p.tokens_spent = 1;
    assert!(p.can_spend_more());
    p.tokens_spent = 2;
    assert!(!p.can_spend_more());
}

// ---- Query ----

#[test]
fn build_query_joins_keyword_with_site_exclusions() {
    let built = build_query("gato montés", &["example.com".to_owned()]);
    assert!(built.query.starts_with("gato montés -site:"));
    assert!(built.query.contains("-site:example.com"));
    assert!(built.query.contains("-site:shutterstock.com"));
    assert_eq!(built.applied + built.dropped, 18);
    assert_eq!(built.dropped, 0);
}

#[test]
fn build_query_never_exceeds_the_word_cap() {
    // Keyword larga + bloqueados disjuntos de la base: se recortan
    // exclusiones por la cola hasta entrar en el tope.
    let keyword = "una keyword deliberadamente larguísima con muchísimas palabras para forzar el recorte de exclusiones del todo";
    let blocked: Vec<String> = (0..40).map(|i| format!("site{i}.example")).collect();
    let built = build_query(keyword, &blocked);
    assert!(built.query.split_whitespace().count() <= MAX_QUERY_WORDS);
    assert!(built.query.starts_with(keyword));
    assert!(built.dropped > 0);
    assert_eq!(built.applied + built.dropped, 17 + 40);
}

#[test]
fn build_query_dedupes_user_domains_against_the_base() {
    let built = build_query(
        "perro",
        &["Shutterstock.com ".to_owned(), "mi-blog.com".to_owned()],
    );
    assert_eq!(built.query.matches("-site:shutterstock.com").count(), 1);
    assert!(built.query.contains("-site:mi-blog.com"));
}

// ---- Filtro ----

#[test]
fn host_parsing_handles_ports_queries_and_case() {
    assert_eq!(
        host_of("https://IMAGES.example.com:8080/a.jpg?x=1#f"),
        Some("images.example.com".to_owned())
    );
    assert_eq!(host_of("http://a.b.c/p.png"), Some("a.b.c".to_owned()));
    assert_eq!(
        host_of("not a url at all"),
        Some("not a url at all".to_owned())
    );
    assert_eq!(host_of(""), None);
}

#[test]
fn host_filter_blocks_substrings_case_insensitively() {
    let blocked = vec!["Shutterstock.com".to_owned()];
    assert!(host_blocked(
        "https://image.shutterstock.com/z.jpg",
        &blocked
    ));
    assert!(host_blocked(
        "https://sub.image.shutterstock.com.evil.com/z.jpg",
        &blocked
    ));
    assert!(!host_blocked("https://example.com/z.jpg", &blocked));
}

#[test]
fn real_photo_extensions_pass_and_junk_fails() {
    assert!(extension_allowed("https://a.com/foto.JPG?v=2"));
    assert!(extension_allowed("https://a.com/foto.jpeg"));
    assert!(extension_allowed("https://a.com/foto.png"));
    assert!(extension_allowed("https://a.com/foto.webp#x"));
    // Un .png puede ser foto real: pasa.
    assert!(!extension_allowed("https://a.com/anim.gif"));
    assert!(!extension_allowed("https://a.com/vector.svg"));
    assert!(!extension_allowed("https://a.com/sin-extension"));
}

#[test]
fn url_patterns_catch_watermarks_previews_and_ai() {
    assert!(url_pattern_blocked("https://thumbs.dreamstime.com/z.jpg"));
    assert!(url_pattern_blocked("https://a.com/comp_image/1.jpg"));
    assert!(url_pattern_blocked("https://a.com/AI-GENERATED-cat.jpg"));
    assert!(!url_pattern_blocked("https://example.com/cat.jpg"));
}

#[test]
fn titles_catch_infographics_vectors_and_mockups() {
    assert!(title_blocked("Cute Cat Infographic (HD Wallpaper)"));
    assert!(title_blocked("FLAT ICON set royalty free"));
    assert!(title_blocked("PowerPoint Template Mockup"));
    assert!(!title_blocked("A tabby cat sleeping in the sun"));
}

#[test]
fn ratio_filter_drops_banners_and_keeps_photos() {
    assert!(ratio_blocked(1200, 300));
    assert!(ratio_blocked(300, 1200));
    assert!(!ratio_blocked(1200, 800));
    assert!(!ratio_blocked(800, 800));
    // Sin dimensiones no se juzga: se conserva.
    assert!(!ratio_blocked(0, 0));
    assert!(!ratio_blocked(640, 0));
}

#[test]
fn classify_applies_extension_host_pattern_title_ratio_in_order() {
    let blocked = default_blocked_domains();
    // Host bloqueado aunque el título sea limpio.
    let p = photo(
        "https://image.shutterstock.com/a.jpg",
        "A cat",
        Some(800),
        Some(600),
    );
    assert_eq!(filter::classify(&p, &blocked), Some(DropReason::Host));
    // Banner con todo lo demás limpio.
    let p = photo("https://example.com/a.jpg", "A cat", Some(1500), Some(400));
    assert_eq!(filter::classify(&p, &blocked), Some(DropReason::Ratio));
    // Foto real pasa.
    let p = photo("https://example.com/a.jpg", "A cat", Some(800), Some(600));
    assert_eq!(filter::classify(&p, &blocked), None);
    // Sin bloqueados del usuario, la base sigue bloqueando stock.
    let p = photo(
        "https://www.gettyimages.com/a.jpg",
        "A cat",
        Some(800),
        Some(600),
    );
    assert_eq!(filter::classify(&p, &no_block()), None);
}

#[test]
fn default_blocked_list_covers_the_spec_families() {
    let blocked = default_blocked_domains();
    for domain in [
        "shutterstock.com",
        "gettyimages.com",
        "vecteezy.com",
        "freepik.com",
        "midjourney.com",
        "tiktok.com",
        "pexels.com",
        "unsplash.com",
        "pixabay.com",
        "deviantart.com",
    ] {
        assert!(blocked.iter().any(|b| b == domain), "{domain} falta");
    }
}

#[test]
fn apply_filter_splits_and_counts() {
    let blocked = default_blocked_domains();
    let photos = vec![
        photo("https://example.com/1.jpg", "Cat", Some(800), Some(600)),
        photo(
            "https://image.shutterstock.com/2.jpg",
            "Cat",
            Some(800),
            Some(600),
        ),
        photo("https://example.com/3.gif", "Cat", Some(800), Some(600)),
        photo(
            "https://example.com/4.jpg",
            "Cat infographic",
            Some(800),
            Some(600),
        ),
        photo("https://example.com/5.jpg", "Cat", Some(2000), Some(500)),
    ];
    let (kept, counts) = apply_filter(photos, &blocked);
    assert_eq!(kept.len(), 1);
    assert_eq!(counts.host, 1);
    assert_eq!(counts.extension, 1);
    assert_eq!(counts.title, 1);
    assert_eq!(counts.ratio, 1);
    assert_eq!(counts.total(), 4);
}

// ---- API: parseo y clave ----

#[test]
fn images_response_parses_serper_fields_with_defaults() {
    let json = r#"{"images": [
        {"title": "Cat", "imageUrl": "https://a.com/1.jpg", "imageWidth": 800, "imageHeight": 600, "link": "https://a.com/p"},
        {"title": "Broken"}
    ]}"#;
    let parsed: ImagesResponse = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.images.len(), 2);
    let photos: Vec<SerperPhoto> = parsed
        .images
        .into_iter()
        .filter_map(|i| i.into_photo())
        .collect();
    assert_eq!(photos.len(), 1);
    assert_eq!(photos[0].id, "https://a.com/1.jpg");
    assert_eq!(photos[0].width, Some(800));
}

#[test]
fn images_response_reads_real_credits_and_falls_back() {
    let json = r#"{"credits": 2, "images": []}"#;
    let parsed: ImagesResponse = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.credits, Some(2));
    // Sin campo `credits` (API antigua): None y el worker estima 2.
    let parsed: ImagesResponse = serde_json::from_str(r#"{"images": []}"#).unwrap();
    assert_eq!(parsed.credits, None);
}

#[test]
fn search_without_key_is_an_error() {
    let had = std::env::var(API_KEY_ENV).ok();
    std::env::remove_var(API_KEY_ENV);
    let err = search("gato", 1, 100, &[]).unwrap_err();
    assert!(err.to_string().contains(API_KEY_ENV), "{err}");
    match had {
        Some(key) => std::env::set_var(API_KEY_ENV, key),
        None => std::env::remove_var(API_KEY_ENV),
    }
}

#[test]
fn search_with_blank_query_returns_empty_without_network() {
    // Sin llamar a la red: la query vacía cortocircuita antes de la clave.
    let page = search("   ", 1, 100, &[]).unwrap();
    assert!(page.photos.is_empty());
    assert!(page.reached_end);
    assert_eq!(page.credits_charged, 0);
}

#[test]
fn decode_turns_png_bytes_into_loaded_image() {
    let img = image::RgbaImage::from_raw(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    )
    .unwrap();
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    let img = decode(&out).unwrap();
    assert_eq!((img.width, img.height), (2, 2));
}

#[test]
fn per_call_constants_match_the_token_model() {
    assert_eq!(PER_CALL_RESULTS, 100);
    assert_eq!(super::api::SMALL_RESULTS, 10);
    assert_eq!(super::api::CREDITS_PER_CALL, 2);
    assert_eq!(super::api::CREDITS_PER_SMALL_CALL, 1);
}

#[test]
fn request_stages_degrade_from_full_to_minimal() {
    let counts = build_query("gato", &["example.com".to_owned()]);
    let full = build_request("gato", &counts, QueryStage::Full, 1, 100);
    assert!(full.body.contains("-site:example.com"));
    assert!(full.body.contains("\"tbs\""));
    assert!(!full.simplified);
    assert_eq!(full.applied, counts.applied);

    let plain = build_request("gato", &counts, QueryStage::PlainKeyword, 2, 100);
    let body: serde_json::Value = serde_json::from_str(&plain.body).unwrap();
    assert_eq!(body["q"], serde_json::Value::String("gato".to_owned()));
    assert!(!plain.body.contains("-site:"));
    assert!(body.get("tbs").is_some());
    assert_eq!(body["page"], serde_json::Value::from(2));
    assert!(plain.simplified);
    assert_eq!(plain.applied, 0);
    assert_eq!(plain.dropped, counts.applied + counts.dropped);

    let minimal = build_request("gato", &counts, QueryStage::Minimal, 1, 100);
    let body: serde_json::Value = serde_json::from_str(&minimal.body).unwrap();
    assert!(body.get("tbs").is_none());
    assert!(minimal.simplified);
}

#[test]
fn request_carries_the_budget_num() {
    // 1 token pide ~10 (1cr); el resto, ~100.
    let counts = build_query("gato", &[]);
    let small = build_request("gato", &counts, QueryStage::Full, 1, 10);
    let body: serde_json::Value = serde_json::from_str(&small.body).unwrap();
    assert_eq!(body["num"], serde_json::Value::from(10));
}

#[test]
fn pattern_error_detection_matches_free_account_rejection() {
    assert!(is_pattern_error(
        "HTTP 400: {\"message\":\"Query pattern not allowed for free accounts.\",\"statusCode\":400}"
    ));
    assert!(!is_pattern_error("HTTP 429: too many requests"));
    assert!(!is_pattern_error("HTTP 400: {\"message\":\"bad request\"}"));
}

// ---- Selección masiva ----

#[test]
fn bulk_open_selects_everything_by_default() {
    let photos = vec![
        super::state::PhotoItem {
            photo: photo("https://a.com/1.jpg", "One", Some(800), Some(600)),
            thumb: None,
            thumb_error: None,
            thumb_requested: true,
        },
        super::state::PhotoItem {
            photo: photo("https://a.com/2.jpg", "Two", Some(800), Some(600)),
            thumb: None,
            thumb_error: None,
            thumb_requested: true,
        },
    ];
    let mut p = Panel {
        photos,
        ..Panel::default()
    };
    p.open_bulk();
    assert!(p.bulk_open);
    assert_eq!(p.bulk_selected.len(), 2);
    p.deselect_all_bulk();
    assert!(p.bulk_selected.is_empty());
    p.select_all_bulk();
    assert_eq!(p.bulk_selected.len(), 2);
}

#[test]
fn masonry_assigns_each_photo_to_the_shortest_column() {
    // La pesada queda sola en una columna y el resto se apila en la otra.
    let cols = bulk_layout::assign_columns(&[3.0, 1.0, 1.0, 1.0], 2);
    assert_eq!(cols.len(), 2);
    assert_eq!(cols[0], vec![0]);
    assert_eq!(cols[1], vec![1, 2, 3]);
    // Sin alturas distintas equivale a round-robin.
    let cols = bulk_layout::assign_columns(&[1.0, 1.0, 1.0], 3);
    assert_eq!(cols, vec![vec![0], vec![1], vec![2]]);
    // Cero columnas no tiene sentido: cae a una.
    let cols = bulk_layout::assign_columns(&[1.0, 2.0], 0);
    assert_eq!(cols.len(), 1);
}

#[test]
fn masonry_aspect_prefers_thumb_over_api_over_fallback() {
    // Thumb manda aunque la API diga otra cosa.
    assert!(
        (bulk_layout::cell_aspect(Some(egui::vec2(800.0, 400.0)), Some(100), Some(100)) - 2.0)
            .abs()
            < 1e-6
    );
    // Sin thumb valen las dims de la API (retrato).
    assert!((bulk_layout::cell_aspect(None, Some(600), Some(1200)) - 0.5).abs() < 1e-6);
    // Sin nada, 4:3; y un thumb degenerado no rompe.
    assert!((bulk_layout::cell_aspect(None, None, None) - 4.0 / 3.0).abs() < 1e-6);
    assert!(
        (bulk_layout::cell_aspect(Some(egui::vec2(0.0, 0.0)), None, None) - 4.0 / 3.0).abs() < 1e-6
    );
}

#[test]
fn cache_key_ignores_case_order_and_whitespace() {
    let a = super::cache::cache_key(
        "  Shakira ",
        &["b.com".to_owned(), "a.com ".to_owned()],
        1,
        100,
    );
    let b = super::cache::cache_key("shakira", &["A.COM".to_owned(), "b.com".to_owned()], 1, 100);
    assert_eq!(a, b);
    // Otra página, otro tamaño u otra keyword sí cambian la clave.
    let c = super::cache::cache_key("shakira", &["a.com".to_owned(), "b.com".to_owned()], 2, 100);
    assert_ne!(a, c);
    let d = super::cache::cache_key("shakira", &["a.com".to_owned(), "b.com".to_owned()], 1, 10);
    assert_ne!(b, d);
}

#[test]
fn disk_cache_round_trips_and_expires() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SERPER_CACHE_DIR", dir.path());
    let key = super::cache::cache_key("shakira", &[], 1, 100);
    assert!(super::cache::load_disk(&key).is_none());
    let page = super::types::SearchPage {
        photos: vec![photo("https://a.com/1.jpg", "One", Some(800), Some(600))],
        reached_end: true,
        credits_charged: 2,
        from_cache: false,
        effective_query: "shakira".to_owned(),
        exclusions_applied: 0,
        exclusions_dropped: 0,
        simplified: false,
        filtered: super::filter::FilterCounts::default(),
    };
    super::cache::save_disk(&key, &page);
    let back = super::cache::load_disk(&key).expect("recién guardada debe leerse");
    assert_eq!(back.photos.len(), 1);
    let live = back.into_page();
    assert!(live.from_cache);
    assert_eq!(live.credits_charged, 0);
    // Caducada no se sirve.
    let mut old = super::cache::CachedPage::from(&page);
    old.saved_at = 0;
    assert!(!old.fresh(u64::MAX));
    std::env::remove_var("SERPER_CACHE_DIR");
}

#[test]
fn memory_cache_holds_twenty_pages_max() {
    let mut p = Panel::default();
    for i in 0..25u32 {
        let key = format!("k{i}");
        let page = super::types::SearchPage {
            photos: Vec::new(),
            reached_end: true,
            credits_charged: 2,
            from_cache: false,
            effective_query: format!("q{i}"),
            exclusions_applied: 0,
            exclusions_dropped: 0,
            simplified: false,
            filtered: super::filter::FilterCounts::default(),
        };
        p.cache_insert(key, &page);
    }
    assert_eq!(p.search_cache.len(), super::cache::MEM_CAP);
    // Las 5 primeras cayeron por LRU; la última sigue.
    assert!(p.cache_lookup("k0").is_none());
    assert!(p.cache_lookup("k24").is_some());
}

#[test]
fn thumb_claims_are_capped_per_frame() {
    let photos = (0..30)
        .map(|i| super::state::PhotoItem {
            photo: photo(
                &format!("https://example.com/{i}.jpg"),
                "P",
                Some(800),
                Some(600),
            ),
            thumb: None,
            thumb_error: None,
            thumb_requested: false,
        })
        .collect();
    let mut p = Panel {
        photos,
        ..Panel::default()
    };
    assert_eq!(p.claim_thumbs(12).len(), 12);
    assert_eq!(p.claim_thumbs(12).len(), 12);
    assert_eq!(p.claim_thumbs(12).len(), 6);
    assert!(p.claim_thumbs(12).is_empty());
}

#[test]
fn thumb_claim_and_retry_cycle() {
    let mut item = super::state::PhotoItem {
        photo: photo("https://a.com/1.jpg", "One", Some(800), Some(600)),
        thumb: None,
        thumb_error: None,
        thumb_requested: false,
    };
    assert!(item.claim_thumb());
    assert!(!item.claim_thumb());
    item.thumb_error = Some("Web download failed: 403".to_owned());
    assert!(!item.claim_thumb());
    item.retry_thumb();
    assert!(item.claim_thumb());
}

#[test]
fn short_reason_summarizes_thumb_failures() {
    assert_eq!(
        super::api::short_reason("Web download failed: 403 Forbidden"),
        "blocked (403)"
    );
    assert_eq!(
        super::api::short_reason("Web image exceeds the 1-byte download limit"),
        "too large"
    );
    assert_eq!(
        super::api::short_reason("Image decode failed: boom"),
        "unreadable"
    );
    assert_eq!(super::api::short_reason("something odd"), "failed");
}

// ---- Horneado CPU (receta de pegado) ----

use bake::{bake_contain_blur, BakeGeom};
use state::{stalled, BULK_STALL_SECS, SEARCH_STALL_SECS};

fn solid_photo(w: u32, h: u32, px: [u8; 4]) -> image::RgbaImage {
    image::RgbaImage::from_pixel(w.max(1), h.max(1), image::Rgba(px))
}

fn geom(pw: u32, ph: u32, fx: f64, fy: f64, fw: f64, fh: f64, has_bg: bool) -> BakeGeom {
    BakeGeom {
        pw,
        ph,
        bg: [255, 255, 255, 255],
        fx,
        fy,
        fw,
        fh,
        has_bg,
    }
}

#[test]
fn bake_cover_photo_is_just_the_photo() {
    // Mismo aspecto que la página y sin fondo: sale la foto tal cual.
    let photo = solid_photo(200, 100, [255, 0, 0, 255]);
    let out = bake_contain_blur(&photo, &geom(200, 100, 0.0, 0.0, 200.0, 100.0, false));
    assert_eq!(out.len(), 200 * 100 * 4);
    assert!(out.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
}

#[test]
fn bake_contain_adds_blurred_bands() {
    // Damero 8×8 rojo/azul sobre página 200×100 con fondo: el blur mezcla
    // las celdas en las bandas (púrpura, ni blanco ni celda nítida) y la
    // foto cae en *contain* 100×100 en x=50..150.
    let mut photo = image::RgbaImage::new(8, 8);
    for (x, y, px) in photo.enumerate_pixels_mut() {
        *px = if (x + y) % 2 == 0 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        };
    }
    let out = bake_contain_blur(&photo, &geom(200, 100, 50.0, 0.0, 100.0, 100.0, true));
    assert_eq!(out.len(), 200 * 100 * 4);
    let px = |x: u32, y: u32| {
        let i = ((y * 200 + x) * 4) as usize;
        [out[i], out[i + 1], out[i + 2], out[i + 3]]
    };
    let band = px(5, 50);
    assert!(band[0] > 50 && band[2] > 50 && band[1] < 50 && band[3] == 255);
    let center = px(100, 50);
    assert!(center[3] == 255 && (center[0] > 50 || center[2] > 50));
}

#[test]
fn bake_empty_photo_yields_background() {
    let photo = image::RgbaImage::new(4, 4);
    let out = bake_contain_blur(&photo, &geom(8, 6, 0.0, 0.0, 8.0, 6.0, true));
    assert_eq!(out.len(), 8 * 6 * 4);
    assert!(out.chunks_exact(4).all(|p| p == [255, 255, 255, 255]));
}

#[test]
fn paste_recipe_adds_blurred_background_on_empty_canvas() {
    // La receta que usa el worker: add_image_layer sobre lienzo vacío con
    // una foto que no cubre → 2 capas, fondo con blur 50.
    let mut state = EditorState::new_blank_image(800.0, 600.0);
    let img = canvas_io::LoadedImage {
        rgba: [255, 0, 0, 255].repeat(100 * 100),
        width: 100,
        height: 100,
    };
    state.add_image_layer("foto", None, img);
    let page = state.doc.page().unwrap();
    assert_eq!(page.layers.len(), 2);
    assert_eq!(page.layers[0].name, "Blurred background");
    assert_eq!(page.layers[0].effects.blur_radius, 50.0);
    // *Contain* de 100×100 en 800×600: 600×600 centrado (toca arriba/abajo).
    assert_eq!(page.layers[1].transform.width.round(), 600.0);
    assert_eq!(page.layers[1].transform.x.round(), 100.0);
}

// ---- Watchdog anti-bloqueo ----

#[test]
fn stalled_detects_lost_flights() {
    use std::time::{Duration, Instant};
    assert!(!stalled(None, Instant::now(), SEARCH_STALL_SECS));
    let old = Instant::now() - Duration::from_secs(SEARCH_STALL_SECS + 1);
    assert!(stalled(Some(old), Instant::now(), SEARCH_STALL_SECS));
    assert!(!stalled(
        Some(Instant::now()),
        Instant::now(),
        BULK_STALL_SECS
    ));
}

#[test]
fn reset_flight_clears_flags_and_ages_seq() {
    let mut p = Panel {
        searching: true,
        bulk_busy: true,
        bulk_progress: Some((3, 85)),
        ..Panel::default()
    };
    let seq = p.search_seq;
    p.reset_flight();
    assert!(!p.searching);
    assert!(!p.bulk_busy);
    assert_eq!(p.bulk_progress, None);
    assert_eq!(p.search_seq, seq + 1);
}

// ---- Humo de render headless ----

/// El panel Web se pinta sin pánico en su estado inicial. Sin clave en el
/// entorno muestra la guía; con clave, la barra de búsqueda.
#[test]
fn panel_ui_renders_without_panic_and_paints() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut state = EditorState::new_blank(800.0, 600.0);
    let mut settings = AppSettings::default();
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
            panel_ui(&mut state.serper, &mut settings, None, ui, &tx);
        },
    );
    assert!(
        !out.shapes.is_empty(),
        "el panel debe pintar su estado inicial"
    );
}

#[test]
fn bulk_destination_prefers_deck_then_file_then_gallery_then_last() {
    use std::path::PathBuf;
    let deck = Some(PathBuf::from("C:/d"));
    let file = Some(PathBuf::from("C:/f/a.png"));
    let gallery = Some(PathBuf::from("C:/g"));
    let last = Some(PathBuf::from("C:/l"));
    assert_eq!(
        bulk::resolve_bulk_folder(deck.clone(), file.clone(), gallery.clone(), last.clone()),
        deck
    );
    assert_eq!(
        bulk::resolve_bulk_folder(None, file.clone(), gallery.clone(), last.clone()),
        Some(PathBuf::from("C:/f"))
    );
    assert_eq!(
        bulk::resolve_bulk_folder(None, None, gallery.clone(), last.clone()),
        gallery
    );
    assert_eq!(
        bulk::resolve_bulk_folder(None, None, None, last.clone()),
        last
    );
    assert_eq!(bulk::resolve_bulk_folder(None, None, None, None), None);
}

#[test]
fn bulk_items_carry_api_dims_for_page_sizing() {
    let mut p = Panel {
        photos: vec![super::state::PhotoItem {
            photo: photo("https://a.com/1.jpg", "One", Some(800), Some(600)),
            thumb: None,
            thumb_error: None,
            thumb_requested: true,
        }],
        ..Panel::default()
    };
    p.select_all_bulk();
    let items = p.selected_bulk_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].width, Some(800));
    assert_eq!(items[0].height, Some(600));
}

/// La ventana masiva se pinta con todo seleccionado y sin pánico.
#[test]
fn bulk_window_renders_with_everything_selected() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut panel = Panel {
        photos: (0..3)
            .map(|i| super::state::PhotoItem {
                photo: photo(
                    &format!("https://example.com/{i}.jpg"),
                    &format!("Photo {i}"),
                    Some(800),
                    Some(600),
                ),
                thumb: None,
                thumb_error: None,
                thumb_requested: true,
            })
            .collect(),
        ..Panel::default()
    };
    panel.open_bulk();
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    // Rect moderado como la ventana por defecto (ya no ocupa el viewport).
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(860.0, 700.0),
            )),
            ..Default::default()
        },
        |ui| {
            bulk::bulk_window_ui(
                &mut panel,
                None,
                &crate::settings::AppSettings::default(),
                ui.ctx(),
                &tx,
            );
        },
    );
    assert!(
        !out.shapes.is_empty(),
        "la ventana masiva debe pintar la cuadrícula"
    );
    assert_eq!(panel.bulk_selected.len(), 3);
}

/// Las tarjetas se pintan sin miniatura (placeholder) y sin pánico.
#[test]
fn photo_cards_render_without_a_thumbnail_and_without_panicking() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut items: Vec<super::state::PhotoItem> = (0..2)
        .map(|i| super::state::PhotoItem {
            photo: photo(
                &format!("https://example.com/{i}.jpg"),
                &format!("Photo {i}"),
                Some(800),
                Some(600),
            ),
            thumb: None,
            thumb_error: None,
            thumb_requested: true,
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
                // `thumb_requested` evita que el humo dispare descargas.
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
