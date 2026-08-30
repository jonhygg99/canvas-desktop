//! Tests de integración GPU para los pipelines de horneado (bake).
//!
//! Replican la lógica de `examples/bake_blur.rs` y `examples/save_roundtrip.rs`
//! pero con imágenes sintéticas en memoria y assertions sobre los píxeles
//! resultantes — no dependen de archivos en disco ni de argumentos CLI.
//!
//! **Requieren GPU real** (wgpu necesita un adaptador). Se marcan
//! `#[ignore]` para que no fallen en CI sin GPU; se corren con:
//!
//! ```sh
//! cargo test -p canvas-render --test gpu_bake -- --ignored
//! ```
//!
//! Cuando hay GPU, verifican regresiones que los tests unitarios de CPU
//! (scene/tests.rs, blur/params.rs) no pueden cubrir: el pipeline completo de
//! GPU (shaders de blur, readback de textura, codificación PNG/JPEG).

use canvas_core::{Document, Effects, ImageContent, LayerContent, Transform};
use canvas_render::{image_data_from_rgba, CanvasRenderer, FxScope, ImageMap};
use vello::util::RenderContext;

/// Crea una imagen RGBA sintética de `w×h` con un degradado rojo→azul
/// horizontal. Determinista (sin aleatoriedad) para que los assertions
/// sobre píxeles sean estables entre ejecuciones.
fn gradient_image(w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = if w > 1 {
                x as f32 / (w - 1) as f32
            } else {
                0.0
            };
            // Rojo a la izquierda, azul a la derecha, verde crece con y.
            rgba.extend_from_slice(&[
                (t * 255.0) as u8,                            // R
                ((y as f32 / h.max(1) as f32) * 255.0) as u8, // G
                ((1.0 - t) * 255.0) as u8,                    // B
                255,                                          // A
            ]);
        }
    }
    (rgba, w, h)
}

/// Crea una imagen sólida de un solo color. Útil para verificar que el
/// desenfoque no cambia píxeles uniformes.
fn solid_image(w: u32, h: u32, [r, g, b, a]: [u8; 4]) -> (Vec<u8>, u32, u32) {
    let rgba: Vec<u8> = (0..w * h).flat_map(|_| [r, g, b, a]).collect();
    (rgba, w, h)
}

/// Imagen de `w×h` con un patrón determinista distinto por `kind` (canales
/// rotados del degradado de `gradient_image`): permite detectar intercambios
/// entre capas al inspeccionar los píxeles horneados.
fn patterned_image(w: u32, h: u32, kind: u8) -> (Vec<u8>, u32, u32) {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = if w > 1 {
                x as f32 / (w - 1) as f32
            } else {
                0.0
            };
            let g = (y as f32 / h.max(1) as f32 * 255.0) as u8;
            let (r, b) = ((t * 255.0) as u8, ((1.0 - t) * 255.0) as u8);
            let [pr, pg, pb] = match kind {
                0 => [r, g, b],
                1 => [b, r, g],
                _ => [g, b, r],
            };
            rgba.extend_from_slice(&[pr, pg, pb, 255]);
        }
    }
    (rgba, w, h)
}

/// Colores únicos a lo largo de una fila horizontal (muestreo cada 8 px). Una
/// región con la capa descartada del atlas de vello sale de un solo color
/// (el fondo de página); una imagen o blur reales dan decenas de colores.
fn row_unique_colors(rgba: &[u8], width: u32, y: u32, x0: u32, x1: u32) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut x = x0;
    while x < x1 {
        let i = ((y * width + x) * 4) as usize;
        seen.insert((rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]));
        x += 8;
    }
    seen.len()
}

/// Obtiene un device/queue wgpu headless. Falla si no hay adaptador
/// (CI sin GPU), lo que hace que los tests `--ignored` fallen ruidosamente
/// en vez de pasar silenciosamente.
fn gpu_device() -> (vello::wgpu::Device, vello::wgpu::Queue) {
    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .expect("no hay adaptador wgpu disponible — este test requiere GPU");
    let handle = &ctx.devices[device_id];
    // Clonamos para que el RenderContext se pueda dropping sin afectar al
    // device (wgpu::Device es Arc internamente, clone es barato).
    (handle.device.clone(), handle.queue.clone())
}

/// Atajo: llama a `bake_page` con el device y queue obtenidos de
/// `gpu_device()`.
fn bake(
    renderer: &mut CanvasRenderer,
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    scope: FxScope,
    doc: &Document,
    images: &ImageMap,
    scale: f64,
) -> (Vec<u8>, u32, u32) {
    renderer
        .bake_page(device, queue, scope, doc, images, scale)
        .unwrap()
}

/// Documento con una capa de imagen a página completa, con blur.
fn doc_with_blur_image(w: u32, h: u32, blur_radius: f32) -> (Document, ImageMap) {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc
        .add_layer(
            "img",
            Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: w,
                natural_height: h,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects = Effects {
        blur_radius,
        ..Default::default()
    };
    let (rgba, iw, ih) = gradient_image(w, h);
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(rgba, iw, ih));
    (doc, images)
}

// ─── bake_blur: horneado básico con blur ───────────────────────────────

/// Replica `bake_blur.rs`: documento con una imagen de degradado + blur,
/// hornea a RGBA8 y verifica dimensiones y que el píxel no sea
/// completamente transparente.
#[test]
#[ignore = "requiere GPU"]
fn bake_blur_produces_non_transparent_pixels() {
    let (device, queue) = gpu_device();
    let (doc, images) = doc_with_blur_image(64, 48, 20.0);

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (rgba, w, h) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );

    assert_eq!(w, 64);
    assert_eq!(h, 48);
    assert_eq!(rgba.len(), (w * h * 4) as usize);

    // Al menos un píxel debe tener alpha > 0 (la imagen llena toda la página).
    let any_visible = rgba.chunks_exact(4).any(|px| px[3] > 0);
    assert!(any_visible, "todos los píxeles son transparentes");
}

/// El horneado con blur produce píxeles distintos del horneado sin blur
/// cuando la imagen tiene contenido variado (degradado). Si los píxeles
/// fueran idénticos, el shader de blur no se estaría aplicando.
#[test]
#[ignore = "requiere GPU"]
fn bake_blur_changes_pixels_vs_no_blur() {
    let (device, queue) = gpu_device();

    // Con blur.
    let (doc_blur, images_blur) = doc_with_blur_image(64, 48, 30.0);
    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (rgba_blur, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc_blur,
        &images_blur,
        1.0,
    );

    // Sin blur (mismos píxeles de origen).
    let (doc_plain, images_plain) = doc_with_blur_image(64, 48, 0.0);
    // FxScope distinto para que la caché de blur no interfiera.
    let (rgba_plain, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope(1),
        &doc_plain,
        &images_plain,
        1.0,
    );

    // Los píxeles deben diferir en algún punto (el blur suaviza el degradado).
    let differ = rgba_blur
        .iter()
        .zip(&rgba_plain)
        .any(|(a, b)| a.abs_diff(*b) > 2);
    assert!(differ, "blur no cambió ningún píxel respecto al original");
}

/// Blur sobre una imagen sólida (color uniforme) no debe cambiar los píxeles
/// — el desenfoque de un color constante es el mismo color. Verifica que el
/// shader no introduce ruido en entradas constantes.
#[test]
#[ignore = "requiere GPU"]
fn bake_blur_on_solid_image_is_identity() {
    let (device, queue) = gpu_device();
    let (w, h) = (32, 32);
    let color = [128, 64, 200, 255];

    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc
        .add_layer(
            "solid",
            Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: w,
                natural_height: h,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects.blur_radius = 25.0;

    let (rgba_src, iw, ih) = solid_image(w, h, color);
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(rgba_src, iw, ih));

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (rgba_out, ow, oh) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );

    assert_eq!((ow, oh), (w, h));
    // Cada píxel de salida debe estar cerca del color de entrada (tolerancia
    // por redondeo del filtro gaussiano en los bordes).
    for px in rgba_out.chunks_exact(4) {
        assert!(
            (px[0] as i32 - color[0] as i32).abs() <= 5
                && (px[1] as i32 - color[1] as i32).abs() <= 5
                && (px[2] as i32 - color[2] as i32).abs() <= 5
                && px[3] == 255,
            "píxel {:?} demasiado lejos del color sólido {color:?}",
            px
        );
    }
}

// ─── save_roundtrip: bake + save + reload ──────────────────────────────

/// Replica `save_roundtrip.rs`: hornea un documento editado (imagen al 50%
/// centrada + fondo blanco + blur) y lo guarda como PNG a un temporal, luego
/// lo reabre y verifica dimensiones y que la esquina sea fondo blanco.
#[test]
#[ignore = "requiere GPU"]
fn save_roundtrip_preserves_edit() {
    let (device, queue) = gpu_device();
    let (w, h) = (64, 48);

    // Imagen sintética de degradado.
    let (rgba_src, iw, ih) = gradient_image(w, h);

    // Documento editado: imagen al 50% centrada + blur + fondo blanco.
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc
        .add_layer(
            "img",
            Transform::new(
                f64::from(w) * 0.25,
                f64::from(h) * 0.25,
                f64::from(w) * 0.5,
                f64::from(h) * 0.5,
            ),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: iw,
                natural_height: ih,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects.blur_radius = 6.0;
    doc.page_mut().unwrap().background = Some([255, 255, 255, 255]);

    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(rgba_src, iw, ih));

    // Hornea.
    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (rgba, bw, bh) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );

    assert_eq!((bw, bh), (w, h));

    // Guardado atómico a un PNG temporal.
    let tmp = std::env::temp_dir().join("canvas_gpu_roundtrip_test.png");
    canvas_io::save_rgba(&tmp, rgba, bw, bh, 92, None).unwrap();

    // Reabre y verifica.
    let saved = image::open(&tmp).unwrap().to_rgba8();
    assert_eq!(saved.dimensions(), (bw, bh));

    // La esquina debe ser fondo blanco (la imagen quedó al 50% centrada).
    let corner = saved.get_pixel(2, 2).0;
    assert_eq!(
        corner,
        [255, 255, 255, 255],
        "la esquina debería ser fondo blanco"
    );

    // El centro debe tener color (la imagen del degradado, aunque borrosa).
    let center = saved.get_pixel(bw / 2, bh / 2).0;
    assert!(center[3] == 255, "el centro debe ser opaco, fue {center:?}");
    // No debe ser blanco puro (hay imagen ahí).
    assert!(
        !(center[0] == 255 && center[1] == 255 && center[2] == 255),
        "el centro no debe ser blanco puro, fue {center:?} — la imagen no se guardó"
    );

    // Limpieza.
    let _ = std::fs::remove_file(&tmp);
}

// ─── open_document → sidecar restaurado → bake: el caso 14.png ─────────

/// El contrato del informe "se exporta en blanco": un documento restaurado
/// desde sidecar (capa con blur, como el fondo de `14.png`) debe hornear con
/// contenido opaco — nunca un PNG blanco. Cubre la composición que ningún
/// otro test toca: `open_document` → `read_sidecar` → `ImageMap` →
/// `bake_page` con scope de ranura NO-default (el de `start_export`/`start_save`).
#[test]
#[ignore = "requiere GPU"]
fn restored_sidecar_document_bakes_with_content() {
    let (device, queue) = gpu_device();
    let dir = std::env::temp_dir().join(format!("canvas_gpu_sidecar_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("foto.png");

    // PNG base blanco (como 14.png) + sidecar con UNA capa desenfocada que
    // cubre la página: suficiente para ejercitar blur + restauración.
    image::RgbaImage::from_pixel(64, 48, image::Rgba([255, 255, 255, 255]))
        .save(&path)
        .unwrap();
    let mut doc = Document::new(64.0, 48.0);
    let bg = doc
        .add_layer(
            "Blurred background",
            Transform::new(0.0, -8.0, 64.0, 60.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 64,
                natural_height: 60,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(bg).unwrap().effects.blur_radius = 50.0;
    let (bg_rgba, ..) = gradient_image(64, 60);
    let payload = canvas_io::CanvasPayload {
        document: doc,
        images: vec![(bg.raw(), bg_rgba, 64, 60)],
        background_layer: Some(bg.raw()),
        preview: None,
    };
    canvas_io::write_sidecar(&path, &std::fs::read(&path).unwrap(), &payload).unwrap();

    // Abre con sidecar (debe restaurar las capas) y hornea con un scope de
    // ranura NO-default, como la app.
    let canvas_io::OpenOutcome::Restored(restored) = canvas_io::open_document(&path, true).unwrap()
    else {
        panic!("se esperaba Restored para una imagen con sidecar válido");
    };
    let mut images = ImageMap::new();
    for (raw, img) in restored.images {
        images.insert(
            canvas_core::LayerId::from_raw(raw),
            image_data_from_rgba(img.rgba, img.width, img.height),
        );
    }
    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let scope = FxScope(7);
    renderer.forget_scope(scope);
    let (rgba, bw, bh) = bake(
        &mut renderer,
        &device,
        &queue,
        scope,
        &restored.document,
        &images,
        1.0,
    );
    assert_eq!((bw, bh), (64, 48));

    // Ni mayoritariamente blanco ni transparente: un visor nunca lo muestra
    // en blanco.
    let mut non_white = 0usize;
    let mut opaque = 0usize;
    for px in rgba.chunks_exact(4) {
        non_white += usize::from(px[..3] != [255, 255, 255]);
        opaque += usize::from(px[3] == 255);
    }
    let n = rgba.len() / 4;
    assert!(
        non_white * 100 / n > 50,
        "el horneado salió mayoritariamente blanco ({non_white}/{n} píxeles no blancos)"
    );
    assert_eq!(opaque, n, "el horneado debe ser opaco por completo");

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── sync_layer + re-bake: caché de efectos GPU ────────────────────────

/// `sync_layer` debe detectar cuando los píxeles de origen cambiaron (vía
/// `Blob::id()`) y re-subir la textura. Este test hornea dos veces con la
/// MISMA `ImageData` (mismo `Blob::id`): la segunda vez debe producir
/// exactamente los mismos píxeles (la caché no debe corromperse).
#[test]
#[ignore = "requiere GPU"]
fn bake_twice_same_source_is_stable() {
    let (device, queue) = gpu_device();
    let (doc, images) = doc_with_blur_image(48, 48, 15.0);

    let mut renderer = CanvasRenderer::new(&device).unwrap();

    let (rgba1, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );
    let (rgba2, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );

    // Los píxeles deben ser idénticos — la caché de blur no debe cambiar
    // el resultado entre frames si los píxeles de origen no cambiaron.
    assert_eq!(
        rgba1, rgba2,
        "el segundo horneado difiere del primero — la caché de sync_layer corrompió el resultado"
    );
}

/// `forget_scope` libera las texturas de efectos de un scope. Después de
/// olvidar, un nuevo horneado debe producir el mismo resultado — los
/// efectos se reconstruyen desde cero.
#[test]
#[ignore = "requiere GPU"]
fn forget_scope_then_bake_reproduces_result() {
    let (device, queue) = gpu_device();
    let (doc, images) = doc_with_blur_image(48, 48, 12.0);

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let scope = FxScope::default();

    let (rgba1, _, _) = bake(&mut renderer, &device, &queue, scope, &doc, &images, 1.0);

    // Olvida el scope (libera texturas de blur).
    renderer.forget_scope(scope);

    // Re-hornea — los efectos se reconstruyen desde cero.
    let (rgba2, _, _) = bake(&mut renderer, &device, &queue, scope, &doc, &images, 1.0);

    // Debe ser idéntico al primer horneado.
    assert_eq!(
        rgba1, rgba2,
        "forget_scope cambió el resultado del horneado — los efectos no se reconstruyeron correctamente"
    );
}

// ─── Escala: horneado a 2x ─────────────────────────────────────────────

/// Horneado a escala 2x produce el doble de píxeles y el contenido es
/// consistente (mismo color de fondo en la esquina).
#[test]
#[ignore = "requiere GPU"]
fn bake_at_2x_produces_double_resolution() {
    let (device, queue) = gpu_device();
    let (w, h) = (32, 24);

    // Documento con fondo blanco y la imagen al 50% centrada (la esquina
    // queda como fondo, no como imagen).
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let (rgba_src, iw, ih) = gradient_image(w, h);
    let id = doc
        .add_layer(
            "img",
            Transform::new(
                f64::from(w) * 0.25,
                f64::from(h) * 0.25,
                f64::from(w) * 0.5,
                f64::from(h) * 0.5,
            ),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: iw,
                natural_height: ih,
                crop: None,
            }),
        )
        .unwrap();
    // Sin blur — la escala debe afectar solo la resolución, no el contenido.
    doc.layer_mut(id).unwrap().effects.blur_radius = 0.0;
    doc.page_mut().unwrap().background = Some([255, 255, 255, 255]);
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(rgba_src, iw, ih));

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (rgba1x, w1, h1) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );
    let (rgba2x, w2, h2) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope(42),
        &doc,
        &images,
        2.0,
    );

    assert_eq!((w1, h1), (32, 24));
    assert_eq!((w2, h2), (64, 48));
    assert_eq!(rgba1x.len(), (32 * 24 * 4) as usize);
    assert_eq!(rgba2x.len(), (64 * 48 * 4) as usize);

    // La esquina es fondo blanco en ambas escalas.
    let c1 = &rgba1x[0..4];
    let c2 = &rgba2x[0..4];
    assert_eq!(c1, &[255, 255, 255, 255]);
    assert_eq!(c2, &[255, 255, 255, 255]);
}

// ─── Imágenes grandes: tope de resolución del atlas de vello ────────────

/// Tres capas de imágenes de 3072×4096 (la central con blur) en un mismo
/// documento. Sin el tope de resolución (`blur::MAX_FX_DIM` = 2048), el total
/// 3×3072 > 8192 desborda el atlas CUADRADO de vello y la última capa se
/// descarta en silencio (`resolve_pending_images` pone su `xy` a `None`) → su
/// región sale con el fondo plano. Con el tope, las tres entran reducidas y
/// se pintan. Es la regresión directa de las fotos verticales de teléfono
/// (≥ 4030 px de alto) que guardaban el blur aplanado o perdían la capa.
///
/// Este test falla SIN el fix y pasa CON él, en cualquier GPU: la caída
/// depende solo de tamaños y del resolutor, no del hardware.
#[test]
#[ignore = "requiere GPU"]
fn bake_three_large_images_all_render() {
    let (device, queue) = gpu_device();
    let (w, h) = (1920u32, 1080u32);
    let (iw, ih) = (3072u32, 4096u32);

    let mut doc = Document::new(f64::from(w), f64::from(h));
    doc.page_mut().unwrap().background = Some([255, 255, 255, 255]);

    let mut images = ImageMap::new();
    for (i, (x, blurred)) in [(0u32, false), (640, true), (1280, false)]
        .into_iter()
        .enumerate()
    {
        let id = doc
            .add_layer(
                format!("img{i}"),
                Transform::new(f64::from(x), 0.0, 640.0, f64::from(h)),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: iw,
                    natural_height: ih,
                    crop: None,
                }),
            )
            .unwrap();
        if blurred {
            doc.layer_mut(id).unwrap().effects.blur_radius = 20.0;
        }
        let (rgba, _, _) = patterned_image(iw, ih, i as u8);
        images.insert(id, image_data_from_rgba(rgba, iw, ih));
    }

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (rgba, bw, bh) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );
    assert_eq!((bw, bh), (w, h));

    let mid = h / 2;
    for (i, (x0, x1)) in [(0, 640), (640, 1280), (1280, 1920)]
        .into_iter()
        .enumerate()
    {
        let uniq = row_unique_colors(&rgba, w, mid, x0, x1);
        assert!(
            uniq > 20,
            "la capa {i} salió plana/blanca ({uniq} colores) — imagen descartada del atlas de vello"
        );
    }
}

/// Réplica del documento real que fallaba (foto vertical de teléfono ≥ 4030
/// px): fondo desenfocado a página completa (blur 50) + la MISMA foto nítida
/// centrada, como en `24.png`. Verifica que ni el fondo ni la foto salen
/// planos al hornear. A diferencia del test anterior, una sola foto entra en
/// el atlas sin el fix (2×3072 ≤ 8192), así que este valida la fidelidad del
/// resultado con el tope activo, no la caída en sí.
#[test]
#[ignore = "requiere GPU"]
fn bake_tall_blur_background_and_sharp_layer_not_flat() {
    let (device, queue) = gpu_device();
    let (w, h) = (1920u32, 1080u32);
    let (iw, ih) = (3072u32, 4096u32);

    let mut doc = Document::new(f64::from(w), f64::from(h));
    doc.page_mut().unwrap().background = Some([255, 255, 255, 255]);

    // Fondo desenfocado: la foto estirada a cubrir la página (0, -740, 1920,
    // 2560), como en el documento real.
    let bg = doc
        .add_layer(
            "Blurred background",
            Transform::new(0.0, -740.0, f64::from(w), 2560.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: iw,
                natural_height: ih,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(bg).unwrap().effects.blur_radius = 50.0;

    // Foto nítida centrada (810×1080 en el documento original).
    let sharp = doc
        .add_layer(
            "Pasted Image",
            Transform::new(555.0, 0.0, 810.0, f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: iw,
                natural_height: ih,
                crop: None,
            }),
        )
        .unwrap();

    let (rgba_src, _, _) = gradient_image(iw, ih);
    let mut images = ImageMap::new();
    // Ambas capas embeben la MISMA foto (como el documento real), una por su
    // `LayerId` propio.
    let src = image_data_from_rgba(rgba_src, iw, ih);
    images.insert(bg, src.clone());
    images.insert(sharp, src);

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (rgba, bw, bh) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );
    assert_eq!((bw, bh), (w, h));

    // Banda superior del fondo (fuera de la foto nítida): un blur real de un
    // degradado conserva decenas de colores; un fondo aplanado tiene 1.
    let bg_uniq = row_unique_colors(&rgba, w, 50, 0, w);
    assert!(
        bg_uniq > 20,
        "el fondo desenfocado salió plano ({bg_uniq} colores)"
    );

    // Centro (rect de la foto nítida): la foto visible, no blanca ni plana.
    let sharp_uniq = row_unique_colors(&rgba, w, h / 2, 555, 1365);
    assert!(
        sharp_uniq > 20,
        "la capa nítida salió plana ({sharp_uniq} colores)"
    );
}

// ─── Aislamiento de scopes entre ventanas ───────────────────────────────

/// Documento a página completa con una capa de imagen de un color sólido y
/// blur. El desenfoque de una imagen uniforme es la misma imagen, así que
/// el color del primer píxel identifica inequívocamente al documento —
/// imprescindible para detectar interferencias entre scopes.
fn doc_with_solid_blur(w: u32, h: u32, rgb: [u8; 3]) -> (Document, ImageMap) {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc
        .add_layer(
            "img",
            Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: w,
                natural_height: h,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects = Effects {
        blur_radius: 50.0,
        ..Default::default()
    };
    let pixels: Vec<u8> = (0..w * h)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(pixels, w, h));
    (doc, images)
}

/// Dos scopes DISTINTOS con el MISMO `LayerId` no interfieren — el caso
/// real de dos ventanas abiertas sobre la misma carpeta (cada `Deck`
/// empieza sus ids de ranura en 1, y el `CanvasRenderer` es compartido).
/// Cada scope debe conservar su propia textura de efectos: hornear A,
/// hornear B, volver a hornear A → A debe seguir siendo la de A (rojo) y
/// B la de B (azul). Si la caché mezclara scopes, el segundo horneado de
/// cada documento dibujaría la textura procesada del otro.
#[test]
#[ignore = "requiere GPU"]
fn distinct_scopes_with_same_layer_id_do_not_interfere() {
    let (device, queue) = gpu_device();
    let (doc_a, images_a) = doc_with_solid_blur(64, 64, [255, 0, 0]); // ventana A: rojo
    let (doc_b, images_b) = doc_with_solid_blur(64, 64, [0, 0, 255]); // ventana B: azul

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let scope_a = FxScope(1001);
    let scope_b = FxScope(2002);

    let (a1, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        scope_a,
        &doc_a,
        &images_a,
        1.0,
    );
    let (b1, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        scope_b,
        &doc_b,
        &images_b,
        1.0,
    );
    // Interleave: cada documento vuelve a hornearse DESPUÉS del otro.
    let (a2, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        scope_a,
        &doc_a,
        &images_a,
        1.0,
    );
    let (b2, _, _) = bake(
        &mut renderer,
        &device,
        &queue,
        scope_b,
        &doc_b,
        &images_b,
        1.0,
    );

    assert!(
        a1[0] > 200 && a1[2] < 60,
        "A debería ser rojo (primer horneado): {:?}",
        &a1[..4]
    );
    assert!(
        b1[2] > 200 && b1[0] < 60,
        "B debería ser azul (primer horneado): {:?}",
        &b1[..4]
    );
    assert!(
        a2[0] > 200 && a2[2] < 60,
        "A cambió tras hornear B: {:?}",
        &a2[..4]
    );
    assert!(
        b2[2] > 200 && b2[0] < 60,
        "B cambió tras volver a hornear A: {:?}",
        &b2[..4]
    );
}

// ─── relevo de texturas al cambiar el tamaño de la fuente ────────────────

/// Réplica del crash real (informe crash-1787704986, foto de 2154×2170): una
/// capa con efectos activos recibe píxeles nuevos de OTRAS dimensiones bajo
/// la MISMA clave `(scope, LayerId)` — p. ej. pegar una foto de otra
/// resolución sobre una capa que ya tenía blur. Antes del relevo,
/// `sync_layer` reescribía los píxeles nuevos EN LAS TEXTURAS VIEJAS y wgpu
/// mataba la app con un error de validación («Copy of X 0..2033 would end up
/// overrunning the bounds of the Destination texture of X size 2000»). Ahora
/// el juego completo se recrea y el horneado refleja la imagen nueva.
#[test]
#[ignore = "requiere GPU"]
fn bake_survives_source_resize_under_same_layer_id() {
    let (device, queue) = gpu_device();
    const PW: u32 = 400;
    const PH: u32 = 300;

    let mut doc = Document::new(f64::from(PW), f64::from(PH));
    let id = doc
        .add_layer(
            "img",
            Transform::new(0.0, 0.0, f64::from(PW), f64::from(PH)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 2000,
                natural_height: 1500,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects.blur_radius = 20.0;

    let mut images = ImageMap::new();
    // Imagen A: 2000×1500 (≤2048: entra sin capar). Las texturas de efectos
    // nacen con ESTE tamaño.
    let (rgba_a, wa, ha) = solid_image(2000, 1500, [200, 30, 30, 255]);
    images.insert(id, image_data_from_rgba(rgba_a, wa, ha));

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (first, fw, fh) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );
    assert_eq!((fw, fh), (PW, PH));

    // Imagen B: 2154×2170 (>2048: se reduce a 2033×2048), azul sólida, bajo
    // LA MISMA clave de caché. Dimensiones distintas de las texturas vivas:
    // aquí estaba el pánico.
    let (rgba_b, wb, hb) = solid_image(2154, 2170, [30, 60, 220, 255]);
    images.insert(id, image_data_from_rgba(rgba_b, wb, hb));

    let (second, sw, sh) = bake(
        &mut renderer,
        &device,
        &queue,
        FxScope::default(),
        &doc,
        &images,
        1.0,
    );
    assert_eq!((sw, sh), (PW, PH));

    // El blur de un color sólido es ese mismo color en todo el rect: centro
    // y esquina deben salir AZULES (imagen B), con alpha opaco.
    let px = |buf: &[u8], x: u32, y: u32| {
        let i = ((y * PW + x) * 4) as usize;
        (buf[i], buf[i + 1], buf[i + 2], buf[i + 3])
    };
    for (x, y) in [(PW / 2, PH / 2), (2, 2)] {
        let (r, g, b, a) = px(&second, x, y);
        assert!(
            r < 80 && g < 110 && b > 160 && a == 255,
            "el horneado tras el relevo no refleja la imagen nueva en ({x},{y}): ({r},{g},{b},{a})"
        );
    }
    assert_ne!(
        px(&first, PW / 2, PH / 2),
        px(&second, PW / 2, PH / 2),
        "el segundo horneado es idéntico al primero: los píxeles nuevos no llegaron a la GPU"
    );
}

// ─── contador de capas omitidas del horneado ─────────────────────────────

/// El guard «anti-incompleto» se apoya en que el bake informa de cuántas
/// capas de imagen/SVG visibles se omitieron al construir la escena. Con
/// todas las imágenes presentes el contador es 0; con una capa ausente del
/// `ImageMap` (píxel que nunca llegó a cargarse) es ≥ 1, sin importar cómo
/// pinte el resultado. La escena no expone esto (la omisión es silenciosa),
/// así que se verifica en el camino de horneado completo, con GPU.
#[test]
#[ignore = "requiere GPU"]
fn bake_reports_skipped_layers_for_missing_source_images() {
    let (device, queue) = gpu_device();
    let mut doc = Document::new(100.0, 100.0);
    let present = doc
        .add_layer(
            "con imagen",
            Transform::new(0.0, 0.0, 50.0, 50.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 4,
                natural_height: 4,
                crop: None,
            }),
        )
        .unwrap();
    let missing = doc
        .add_layer(
            "sin imagen",
            Transform::new(50.0, 50.0, 50.0, 50.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 4,
                natural_height: 4,
                crop: None,
            }),
        )
        .unwrap();

    // Solo la primera capa tiene píxeles en el mapa: la segunda se omite al
    // construir la escena y el bake debe informarla.
    let (rgba, w, h) = solid_image(4, 4, [200, 30, 30, 255]);
    let mut images = ImageMap::new();
    images.insert(present, image_data_from_rgba(rgba, w, h));

    let mut renderer = CanvasRenderer::new(&device).unwrap();
    let (_, bw, bh, skipped) = renderer
        .bake_page_counting(&device, &queue, FxScope::default(), &doc, &images, 1.0)
        .unwrap();
    assert_eq!((bw, bh), (100, 100));
    assert_eq!(skipped, 1, "la capa sin píxel debe contarse como omitida");

    // Contraprueba: con todas las imágenes presentes, el contador vuelve a 0.
    let (rgba2, w2, h2) = solid_image(4, 4, [30, 60, 220, 255]);
    images.insert(missing, image_data_from_rgba(rgba2, w2, h2));
    let (_, _, _, skipped2) = renderer
        .bake_page_counting(&device, &queue, FxScope::default(), &doc, &images, 1.0)
        .unwrap();
    assert_eq!(
        skipped2, 0,
        "con todas las imágenes presentes no hay omitidas"
    );
}
