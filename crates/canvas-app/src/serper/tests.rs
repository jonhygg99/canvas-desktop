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
        thumb_url: None,
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
    assert_eq!(
        filter::classify(&p, &blocked, false),
        Some(DropReason::Host)
    );
    // Banner con todo lo demás limpio.
    let p = photo("https://example.com/a.jpg", "A cat", Some(1500), Some(400));
    assert_eq!(
        filter::classify(&p, &blocked, false),
        Some(DropReason::Ratio)
    );
    // Foto real pasa.
    let p = photo("https://example.com/a.jpg", "A cat", Some(800), Some(600));
    assert_eq!(filter::classify(&p, &blocked, false), None);
    // Sin bloqueados del usuario, la base sigue bloqueando stock.
    let p = photo(
        "https://www.gettyimages.com/a.jpg",
        "A cat",
        Some(800),
        Some(600),
    );
    assert_eq!(filter::classify(&p, &no_block(), false), None);
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
    let (kept, counts) = apply_filter(photos, &blocked, false);
    assert_eq!(kept.len(), 1);
    assert_eq!(counts.host, 1);
    assert_eq!(counts.extension, 1);
    assert_eq!(counts.title, 1);
    assert_eq!(counts.ratio, 1);
    assert_eq!(counts.total(), 4);
}

#[test]
fn social_forgives_only_social_cdns() {
    use types::SocialNetwork;
    let blocked = default_blocked_domains();
    // CDN social: cae en web, pasa en social.
    let p = photo(
        "https://lookaside.instagram.com/seo/google_widget/c/ig.jpg",
        "Shakira",
        Some(800),
        Some(600),
    );
    assert_eq!(
        filter::classify(&p, &blocked, false),
        Some(DropReason::Host)
    );
    assert_eq!(filter::classify(&p, &blocked, true), None);
    let p = photo(
        "https://scontent.fbsbx.com/v/photo.jpg",
        "Shakira",
        Some(800),
        Some(600),
    );
    assert_eq!(filter::classify(&p, &blocked, true), None);
    // Stock con marca sigue cayendo también en social.
    let p = photo(
        "https://image.shutterstock.com/a.jpg",
        "Shakira",
        Some(800),
        Some(600),
    );
    assert_eq!(filter::classify(&p, &blocked, true), Some(DropReason::Host));
    // Sin extensión en la URL: cae en web, pasa en social (lo decide el
    // decode del thumb).
    let p = photo(
        "https://lookaside.instagram.com/seo/google_widget/c/abc123",
        "Shakira",
        Some(800),
        Some(600),
    );
    assert_eq!(
        filter::classify(&p, &blocked, false),
        Some(DropReason::Extension)
    );
    assert_eq!(filter::classify(&p, &blocked, true), None);
    // Títulos siguen filtrando en social.
    let p = photo(
        "https://lookaside.instagram.com/a.jpg",
        "Shakira infographic",
        Some(800),
        Some(600),
    );
    assert_eq!(
        filter::classify(&p, &blocked, true),
        Some(DropReason::Title)
    );
    let _ = SocialNetwork::Instagram;
}

#[test]
fn person_link_parses_profiles_posts_and_names() {
    use types::{PersonQuery, SocialNetwork};
    assert_eq!(
        filter::parse_person_link("https://www.instagram.com/shakira/"),
        Some(PersonQuery {
            network: Some(SocialNetwork::Instagram),
            handle: "shakira".to_owned(),
        })
    );
    assert_eq!(
        filter::parse_person_link("instagram.com/shakira?igsh=abc"),
        Some(PersonQuery {
            network: Some(SocialNetwork::Instagram),
            handle: "shakira".to_owned(),
        })
    );
    assert!(filter::parse_person_link("https://www.instagram.com/p/C123abc/").is_none());
    assert!(filter::parse_person_link("https://www.instagram.com/").is_none());
    assert_eq!(
        filter::parse_person_link("https://www.facebook.com/shakira/photos"),
        Some(PersonQuery {
            network: Some(SocialNetwork::Facebook),
            handle: "shakira".to_owned(),
        })
    );
    assert_eq!(
        filter::parse_person_link("https://www.facebook.com/profile.php?id=12345"),
        Some(PersonQuery {
            network: Some(SocialNetwork::Facebook),
            handle: "12345".to_owned(),
        })
    );
    assert_eq!(
        filter::parse_person_link("Shakira"),
        Some(PersonQuery {
            network: None,
            handle: "Shakira".to_owned(),
        })
    );
    assert_eq!(filter::parse_person_link("   "), None);
}

#[test]
fn person_query_prefers_site_operator() {
    use types::{PersonQuery, SocialNetwork};
    let queries = filter::build_person_query(&PersonQuery {
        network: Some(SocialNetwork::Instagram),
        handle: "shakira".to_owned(),
    });
    assert_eq!(queries.full, "site:instagram.com \"shakira\"");
    // El degradado gratuito no lleva ningún operador.
    assert_eq!(queries.plain, "shakira");
    assert!(!queries.plain.contains("site:"));
    let queries = filter::build_person_query(&PersonQuery {
        network: None,
        handle: "Shakira".to_owned(),
    });
    assert_eq!(queries.full, "Shakira");
    assert_eq!(queries.plain, "Shakira");
}

#[test]
fn degrade_chain_drops_site_operator() {
    // El link genera Full con `site:`, pero las etapas degradadas usan el
    // handle pelado: las gratuitas rechazan `site:` igual que `-site:`.
    use types::{PersonQuery, SocialNetwork};
    let queries = filter::build_person_query(&PersonQuery {
        network: Some(SocialNetwork::Instagram),
        handle: "anapatriciatv".to_owned(),
    });
    let counts = filter::build_query(&queries.full, &[]);
    let plain = api::build_request(
        &queries.plain,
        &counts,
        api::QueryStage::PlainKeyword,
        1,
        10,
    );
    let minimal = api::build_request(&queries.plain, &counts, api::QueryStage::Minimal, 1, 10);
    for body in [&plain.body, &minimal.body] {
        assert!(!body.contains("site:"), "{body}");
    }
    let full = api::build_request(&queries.plain, &counts, api::QueryStage::Full, 1, 100);
    assert!(full.body.contains("site:instagram.com"), "{}", full.body);
}

#[test]
fn keep_network_keeps_only_the_detected_network() {
    use types::SocialNetwork;
    fn ig(path: &str) -> SerperPhoto {
        let mut p = photo(
            "https://lookaside.instagram.com/x.jpg",
            "Shakira",
            Some(800),
            Some(600),
        );
        p.source_url = format!("https://www.instagram.com/{path}");
        p
    }
    fn other(url: &str, source: &str) -> SerperPhoto {
        let mut p = photo(url, "Shakira", Some(800), Some(600));
        p.source_url = source.to_owned();
        p
    }
    let photos = vec![
        ig("p/C123/"),
        other("https://pbs.twimg.com/a.jpg", "https://x.com/u/status/1"),
        other(
            "https://example.com/a.jpg",
            "https://www.threads.com/@u/post/1",
        ),
        other(
            "https://scontent.fbsbx.com/b.jpg",
            "https://www.facebook.com/photo/1",
        ),
    ];
    // Link de Instagram: solo IG.
    let (kept, dropped) = filter::keep_network(photos.clone(), Some(SocialNetwork::Instagram));
    assert_eq!(kept.len(), 1);
    assert_eq!(dropped, 3);
    // Nombre plano: IG + FB, fuera threads/X.
    let (kept, dropped) = filter::keep_network(photos, None);
    assert_eq!(kept.len(), 2);
    assert_eq!(dropped, 2);
}

#[test]
fn switch_mode_clears_results_and_spend() {
    let mut p = Panel::default();
    p.switch_mode(types::SearchMode::Social);
    assert_eq!(p.mode, types::SearchMode::Social);
    // Cambiar al mismo modo no toca nada.
    p.tokens_spent = 2;
    p.switch_mode(types::SearchMode::Social);
    assert_eq!(p.tokens_spent, 2);
    // Volver a web limpia el gasto de la keyword.
    p.switch_mode(types::SearchMode::Web);
    assert_eq!(p.mode, types::SearchMode::Web);
    assert_eq!(p.tokens_spent, 0);
    assert!(p.photos.is_empty());
    assert!(!p.searching);
}

// ---- API: parseo y clave ----

#[test]
fn images_response_parses_serper_fields_with_defaults() {
    let json = r#"{"images": [
        {"title": "Cat", "imageUrl": "https://a.com/1.jpg", "imageWidth": 800, "imageHeight": 600, "link": "https://a.com/p", "thumbnailUrl": "https://encrypted-tbn0.gstatic.com/tbn.jpg"},
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
    assert_eq!(
        photos[0].thumb_url,
        Some("https://encrypted-tbn0.gstatic.com/tbn.jpg".to_owned())
    );
}

#[test]
fn thumb_source_prefers_google_proxy() {
    let mut p = photo("https://a.com/1.jpg", "Cat", Some(800), Some(600));
    assert_eq!(p.thumb_source(), "https://a.com/1.jpg");
    p.thumb_url = Some("https://encrypted-tbn0.gstatic.com/tbn.jpg".to_owned());
    assert_eq!(
        p.thumb_source(),
        "https://encrypted-tbn0.gstatic.com/tbn.jpg"
    );
    // Vacía o en blanco: fallback a la original.
    p.thumb_url = Some("   ".to_owned());
    assert_eq!(p.thumb_source(), "https://a.com/1.jpg");
}

#[test]
fn old_cached_photos_without_thumb_url_still_parse() {
    // Compat con cachés de disco anteriores al campo `thumb_url`.
    let json = r#"{"id":"https://a.com/1.jpg","title":"Cat","image_url":"https://a.com/1.jpg","width":800,"height":600,"source_url":"https://a.com/p"}"#;
    let photo: SerperPhoto = serde_json::from_str(json).unwrap();
    assert_eq!(photo.thumb_url, None);
    assert_eq!(photo.thumb_source(), "https://a.com/1.jpg");
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
    let err = search("gato", 1, 100, &[], types::SearchMode::Web).unwrap_err();
    assert!(err.to_string().contains(API_KEY_ENV), "{err}");
    match had {
        Some(key) => std::env::set_var(API_KEY_ENV, key),
        None => std::env::remove_var(API_KEY_ENV),
    }
}

#[test]
fn search_with_blank_query_returns_empty_without_network() {
    // Sin llamar a la red: la query vacía cortocircuita antes de la clave.
    let page = search("   ", 1, 100, &[], types::SearchMode::Web).unwrap();
    assert!(page.photos.is_empty());
    assert!(page.reached_end);
    assert_eq!(page.credits_charged, 0);
}

#[test]
fn social_referer_matches_cdn_family() {
    assert_eq!(
        super::api::social_referer("https://lookaside.instagram.com/x.jpg"),
        Some("https://www.instagram.com/")
    );
    assert_eq!(
        super::api::social_referer("https://scontent-mad1-1.xx.fbcdn.net/y.jpg"),
        Some("https://www.instagram.com/")
    );
    assert_eq!(
        super::api::social_referer("https://scontent.fbsbx.com/z.jpg"),
        Some("https://www.instagram.com/")
    );
    assert_eq!(
        super::api::social_referer("https://www.facebook.com/photo.jpg"),
        Some("https://www.facebook.com/")
    );
    assert_eq!(
        super::api::social_referer("https://www.tiktok.com/@u/video/1"),
        Some("https://www.tiktok.com/")
    );
    // Hosts normales: sin cabeceras extra (comportamiento actual).
    assert_eq!(
        super::api::social_referer("https://example.com/a.jpg"),
        None
    );
    assert_eq!(super::api::social_referer("not a url"), None);
}

#[test]
fn og_image_url_extracts_embed_photo() {
    assert_eq!(
        super::api::og_image_url(
            r#"<html><head><meta property="og:image" content="https://scontent-mad1-1.xx.fbcdn.net/a.jpg?x=1" /></head>"#
        ),
        Some("https://scontent-mad1-1.xx.fbcdn.net/a.jpg?x=1".to_owned())
    );
    // Atributos al revés y comillas simples.
    assert_eq!(
        super::api::og_image_url(
            r#"<meta content='https://scontent-mad1-1.xx.fbcdn.net/b.jpg' property='og:image'>"#
        ),
        Some("https://scontent-mad1-1.xx.fbcdn.net/b.jpg".to_owned())
    );
    // Entidades HTML en la query.
    assert_eq!(
        super::api::og_image_url(
            r#"<meta property="og:image" content="https://scontent.xx.fbcdn.net/c.jpg?stp=dst-jpg&amp;x=1" />"#
        ),
        Some("https://scontent.xx.fbcdn.net/c.jpg?stp=dst-jpg&x=1".to_owned())
    );
    // Ignora otros metas y devuelve None si no hay og:image.
    assert_eq!(
        super::api::og_image_url(
            r#"<meta name="description" content="https://x.test/a.jpg"><title>t</title>"#
        ),
        None
    );
    assert_eq!(super::api::og_image_url("not html at all"), None);
    assert_eq!(
        super::api::og_image_url(r#"<meta property="og:image" content="">"#),
        None
    );
}

#[test]
fn fetched_image_reports_when_its_pixels_came_from_another_url() {
    // A08: si la directa falla y el rescate trae otra URL (p. ej. el
    // `og:image` B de una página social cuando se pidió A), el resultado lo
    // declara en vez de pasar por la foto elegida.
    fn fetched(resolved: &str) -> FetchedImage {
        FetchedImage {
            image: canvas_io::LoadedImage {
                rgba: vec![255, 0, 0, 255],
                width: 1,
                height: 1,
            },
            resolved_url: resolved.to_owned(),
        }
    }
    let direct = "https://scontent.xx.fbcdn.net/a.jpg";
    assert!(
        !fetched(direct).substituted(direct),
        "la directa no sustituye"
    );
    let rescued = "https://scontent.xx.fbcdn.net/b.jpg";
    assert!(
        fetched(rescued).substituted(direct),
        "el og:image B debe marcarse como sustitución de A"
    );
    // La miniatura ya mostrada también cuenta como sustitución si difiere.
    let thumb = "https://encrypted-tbn0.gstatic.com/tbn.jpg";
    assert!(fetched(thumb).substituted(direct));
}

#[test]
#[ignore = "live: gasta ~1cr de la key real; borrar tras verificar"]
fn live_social_thumb_downloads_with_browser_headers() {
    use types::SearchMode;
    let page = search(
        "https://www.instagram.com/anapatriciatv/",
        1,
        10,
        &[],
        SearchMode::Social,
    )
    .expect("la búsqueda social debe funcionar");
    let first = page.photos.first().expect("debe traer fotos");
    eprintln!("LIVE url={}", first.image_url);
    // Vía A (previews): thumbnail proxy de Google.
    let thumb = first.thumb_source();
    eprintln!("LIVE thumb={thumb}");
    let tbytes = super::api::download(&thumb).expect("el thumb proxy debe descargar");
    let timg = decode(&tbytes).expect("el thumb debe decodificar");
    assert!(timg.width > 0 && timg.height > 0);
    eprintln!("LIVE thumb OK: {}x{}", timg.width, timg.height);
    // Exploro source_urls de la tanda.
    for p in page.photos.iter().take(10) {
        eprintln!("LIVE source={}", p.source_url);
    }
    // Vía B: página del perfil/post con UA de crawler de Meta.
    let crawler_bytes = crate::http::get_bytes_with_headers(
        &first.source_url,
        crate::http::MAX_DOWNLOAD_BYTES,
        &[("User-Agent", "facebookexternalhit/1.1")],
    )
    .expect("la página debe descargar como crawler");
    let crawler_html = String::from_utf8_lossy(&crawler_bytes);
    eprintln!(
        "LIVE crawler_len={} has_og={}",
        crawler_html.len(),
        crawler_html.contains("og:image")
    );
    let og = super::api::og_image_url(&crawler_html).expect("el crawler debe ver og:image");
    eprintln!("LIVE og={og}");
    let fetched = fetch_image(
        &first.image_url,
        Some(&first.source_url),
        first.thumb_url.as_deref(),
    )
    .expect("el full debe resolver");
    eprintln!("LIVE resolved={}", fetched.resolved_url);
    let img = fetched.image;
    assert!(img.width > 0 && img.height > 0);
    eprintln!("LIVE OK: {}x{}", img.width, img.height);
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
fn frozen_bulk_layout_survives_new_thumb_heights() {
    // A02: con alturas iniciales iguales el reparto es [[A,C],[B,D]]; si
    // llega un thumb alto para A, el reparto fresco sería [[A],[B,C,D]]
    // (C ocuparía el sitio de D). El reparto congelado por ids debe
    // seguir mapeando cada id a su columna original.
    use super::bulk::{bulk_ids_match, bulk_ids_to_indices, bulk_indices_to_ids};
    use super::bulk_layout::assign_columns;
    let ids = vec!["A", "B", "C", "D"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let fresh = assign_columns(&[100.0, 100.0, 100.0, 100.0], 2);
    assert_eq!(fresh, vec![vec![0, 2], vec![1, 3]]);
    let frozen = bulk_indices_to_ids(&fresh, &ids);
    // Mismo conjunto de fotos: el reparto congelado sigue valiendo aunque
    // las alturas hayan cambiado.
    assert!(bulk_ids_match(&frozen, &ids));
    let reused = bulk_ids_to_indices(&frozen, &ids);
    assert_eq!(reused, fresh, "a mitad de un gesto no se recoloca");
    // El reparto fresco con la altura nueva SÍ recolocaría (lo que se evita).
    let moved = assign_columns(&[400.0, 100.0, 100.0, 100.0], 2);
    assert_ne!(
        moved, fresh,
        "el caso de la auditoría debe recolocar en fresco"
    );
    // Si se retira una foto, el reparto congelado deja de valer.
    let ids3 = vec!["A".to_owned(), "B".to_owned(), "D".to_owned()];
    assert!(!bulk_ids_match(&frozen, &ids3));
}

#[test]
fn insert_target_only_matches_its_own_destination() {
    // A07: la respuesta solo vale si la petición pendiente es esta misma Y
    // la baraja sigue en la misma generación con la misma ranura activa.
    use crate::loader::{insert_target_current, ImageInsertDest, ImageInsertTarget};
    let dest = ImageInsertDest {
        generation: 7,
        slot_id: 3,
    };
    let target = ImageInsertTarget {
        dest,
        seq: 0,
        photo_id: "A".to_owned(),
    };
    let pending = Some(target.clone());
    assert!(insert_target_current(pending.as_ref(), &target, 7, Some(3)));
    // Salto de lienzo: otra ranura.
    assert!(!insert_target_current(
        pending.as_ref(),
        &target,
        7,
        Some(5)
    ));
    // Proyecto nuevo: otra generación.
    assert!(!insert_target_current(
        pending.as_ref(),
        &target,
        8,
        Some(3)
    ));
    // Sin ranura activa: no hay destino vigente.
    assert!(!insert_target_current(pending.as_ref(), &target, 7, None));
    // Sin petición pendiente (panel nuevo tras reabrir): caducada.
    assert!(!insert_target_current(None, &target, 7, Some(3)));
    // Otra foto con el mismo `seq` es otra petición.
    let other = ImageInsertTarget {
        photo_id: "B".to_owned(),
        ..target.clone()
    };
    assert!(!insert_target_current(pending.as_ref(), &other, 7, Some(3)));
}

#[test]
fn begin_insert_seals_one_request_at_a_time() {
    // A07: el sellado reserva un `seq` nuevo por petición y bloquea una
    // segunda mientras la primera sigue en vuelo.
    let mut panel = Panel::default();
    let dest = crate::loader::ImageInsertDest {
        generation: 7,
        slot_id: 3,
    };
    let first = panel.begin_insert(dest, "A").expect("primera petición");
    assert_eq!(first.seq, 0);
    assert_eq!(panel.inserting.as_deref(), Some("A"));
    assert!(panel.begin_insert(dest, "B").is_none(), "en vuelo: no pisa");
    assert_eq!(panel.inserting.as_deref(), Some("A"), "sigue la primera");
}

#[test]
fn bring_more_paginates_the_sealed_spec_not_the_draft() {
    // A09: buscar A, escribir B sin pulsar Search y pedir más pagina A
    // (página 2) — nunca mezcla la página 2 de B con los resultados de A.
    use super::panel::{spend_token, start_search};
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = egui::Context::default();
    let mut panel = Panel {
        query: "gatos".to_owned(),
        ..Panel::default()
    };
    let num = panel.budget.num();
    let mode = types::SearchMode::Web;
    fn page_with(url: &str) -> super::types::SearchPage {
        super::types::SearchPage {
            photos: vec![photo(url, "T", Some(800), Some(600))],
            reached_end: false,
            credits_charged: 2,
            from_cache: false,
            effective_query: url.to_owned(),
            exclusions_applied: 0,
            exclusions_dropped: 0,
            simplified: false,
            filtered: super::filter::FilterCounts::default(),
        }
    }
    // Página 1 de "gatos" en caché: el Search se sirve sin red y sella.
    panel.cache_insert(
        super::cache::cache_key("gatos", &[], 1, num, mode),
        &page_with("https://a.com/gato1.jpg"),
    );
    start_search(&mut panel, &[], &tx, &ctx);
    assert_eq!(
        panel.active_search.as_ref().map(|s| s.keyword.as_str()),
        Some("gatos"),
        "el Search sella la spec"
    );
    assert_eq!(panel.page, 1);
    // El usuario escribe "perros" SIN pulsar Search...
    panel.query = "perros".to_owned();
    // ...y pide más: página 2 de "gatos" (en caché), sin gastar ni mezclar.
    panel.cache_insert(
        super::cache::cache_key("gatos", &[], 2, num, mode),
        &page_with("https://a.com/gato2.jpg"),
    );
    spend_token(&mut panel, &tx, &ctx);
    assert_eq!(panel.page, 2);
    assert_eq!(panel.query, "perros", "el borrador no se toca");
    let ids: Vec<&str> = panel.photos.iter().map(|p| p.photo.id.as_str()).collect();
    assert!(
        ids.contains(&"https://a.com/gato1.jpg") && ids.contains(&"https://a.com/gato2.jpg"),
        "páginas 1+2 de gatos: {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id.contains("perros")),
        "nada de perros mezclado: {ids:?}"
    );
    assert_eq!(panel.tokens_spent, 0, "de caché: sin gasto");
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
    use types::SearchMode;
    let a = super::cache::cache_key(
        "  Shakira ",
        &["b.com".to_owned(), "a.com ".to_owned()],
        1,
        100,
        SearchMode::Web,
    );
    let b = super::cache::cache_key(
        "shakira",
        &["A.COM".to_owned(), "b.com".to_owned()],
        1,
        100,
        SearchMode::Web,
    );
    assert_eq!(a, b);
    // Otra página, otro tamaño, otro modo u otra keyword sí cambian la clave.
    let c = super::cache::cache_key(
        "shakira",
        &["a.com".to_owned(), "b.com".to_owned()],
        2,
        100,
        SearchMode::Web,
    );
    assert_ne!(a, c);
    let d = super::cache::cache_key(
        "shakira",
        &["a.com".to_owned(), "b.com".to_owned()],
        1,
        10,
        SearchMode::Web,
    );
    assert_ne!(b, d);
    let s = super::cache::cache_key(
        "shakira",
        &["b.com".to_owned(), "a.com ".to_owned()],
        1,
        100,
        SearchMode::Social,
    );
    assert_ne!(a, s);
}

#[test]
fn disk_cache_round_trips_and_expires() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SERPER_CACHE_DIR", dir.path());
    let key = super::cache::cache_key("shakira", &[], 1, 100, types::SearchMode::Web);
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
    // Con llegadas entre frames (producción: cada respuesta libera su plaza
    // con `note_thumb_arrived`), el tope por frame manda: 12/12/6/vacío.
    for expected in [12, 12, 6, 0] {
        let claimed = p.claim_thumbs(12);
        assert_eq!(claimed.len(), expected);
        for (id, _, _) in claimed {
            p.note_thumb_arrived(&id);
        }
    }
}

#[test]
fn thumb_inflight_cap_bounds_simultaneous_downloads() {
    // A11: SIN llegadas, el segundo frame no reclama más allá del tope
    // simultáneo aunque queden fotos y presupuesto por frame.
    use super::state::MAX_THUMB_INFLIGHT;
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
    assert_eq!(
        p.claim_thumbs(12).len(),
        MAX_THUMB_INFLIGHT - 12,
        "el tope simultáneo frena aunque haya presupuesto por frame"
    );
    assert!(p.claim_thumbs(12).is_empty(), "cola llena: nada más");
    // Al llegar la mitad, se liberan plazas.
    let arrived: Vec<String> = p.thumb_inflight.iter().take(6).cloned().collect();
    for id in arrived {
        p.note_thumb_arrived(&id);
    }
    assert_eq!(p.claim_thumbs(12).len(), 6);
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
    assert!(out
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 0, 0, 255]));
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
    assert!(out
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 255, 255, 255]));
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
            let insert_dest = crate::loader::ImageInsertDest {
                generation: 0,
                slot_id: 0,
            };
            panel_ui(&mut state.serper, &mut settings, None, insert_dest, ui, &tx);
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
