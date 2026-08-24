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
