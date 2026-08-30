//! Construccion de la escena vello a partir del documento.

use std::sync::{Arc, OnceLock};

use canvas_core::Document;
use vello::kurbo::Affine;
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use vello::Scene;

mod document;
mod raster;
mod shape;
mod text;

#[cfg(test)]
mod tests;

pub use raster::{image_data_from_rgba, ImageMap};
pub use text::text_lines;

/// Píxel de 1x1 transparente que se dibuja (fuera de página) en TODA escena
/// que se mande al renderizador compartido. Ver `draw_atlas_anchor`.
pub fn atlas_anchor() -> &'static ImageData {
    static ANCHOR: OnceLock<ImageData> = OnceLock::new();
    ANCHOR.get_or_init(|| ImageData {
        data: Blob::new(Arc::new([0u8; 4])),
        format: ImageFormat::Rgba8,
        width: 1,
        height: 1,
        alpha_type: ImageAlphaType::Alpha,
    })
}

/// Garantiza que `scene` tenga al menos UN patch de imagen. Vello 0.9, ante
/// una escena sin patches (`Resolver::resolve` vuelve pronto con
/// `Images::default()`, de ancho 0), redimensiona el proxy GPU del atlas de
/// imágenes a 1x1: LIBERA la textura grande y crea una vacía. La caché CPU
/// del atlas no se entera — las imágenes ya residentes conservan sus
/// coordenadas con `dirty=false`, nunca se re-suben, y todos los lienzos con
/// fotos quedan en blanco hasta que algo fuerce una re-subida (volver a la
/// galería recrea el renderer; un efecto que cambie marca la suya).
///
/// El disparador real del bug reportado: guardar un lienzo SOLO vectorial
/// (p. ej. un canvas nuevo con un triángulo) hornea una escena sin ninguna
/// imagen y, al compartir renderer con la baraja viva, borra el atlas de
/// TODAS las fotos. También bastaba con que la cámara quedara un frame entre
/// páginas sin ninguna imagen a la vista. Dibujar este píxel invisible en
/// cada escena hace que nunca sea patchless: el atlas GPU se conserva.
pub fn draw_atlas_anchor(scene: &mut Scene) {
    scene.draw_image(atlas_anchor(), Affine::translate((-1.0e9, -1.0e9)));
}

/// Construye la escena de UN documento con la transformación de vista dada
/// (página → píxeles físicos del lienzo). Envoltorio de una sola llamada a
/// `append_document`: mismo resultado byte a byte que antes de que existiera.
pub fn build_scene(
    doc: &Document,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    decorated: bool,
) -> Scene {
    let mut scene = Scene::new();
    append_document(&mut scene, doc, images, blurred, view, decorated);
    scene
}

/// Los píxeles con los que pintar una capa: la textura ya procesada (efectos
/// GPU) si la hay, y si no la original del documento. `None` mientras la carga
/// asíncrona no ha terminado, o si el mapa de bits mide 0x0.
///
/// Que devuelva `Option` en vez de que el llamador corte por lo sano con un
/// `continue` es deliberado: el bucle de `append_document` lleva su propio
/// índice y cierra capas de opacidad al final de cada vuelta, así que saltarse
/// el resto del cuerpo lo dejaría sin avanzar y sin cerrar.
fn drawable_image<'a>(
    blurred: &'a ImageMap,
    images: &'a ImageMap,
    id: canvas_core::LayerId,
) -> Option<&'a vello::peniko::ImageData> {
    let image = blurred.get(&id).or_else(|| images.get(&id))?;
    (image.width != 0 && image.height != 0).then_some(image)
}

/// Añade UN documento a una escena ya empezada, con su propia transformación
/// de vista. El editor multi-lienzo llama a esto una vez por lienzo visible
/// de la baraja, todos en la MISMA escena (un solo `CanvasSurface`). `blurred`
/// sustituye la imagen de las capas con desenfoque activo (textura GPU ya
/// procesada).
///
/// Con `decorated` se pintan los adornos de edición (tablero de transparencia
/// y borde de página); el horneado para guardar/exportar va sin ellos.
pub fn append_document(
    scene: &mut Scene,
    doc: &Document,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    decorated: bool,
) {
    document::append_document(scene, doc, images, blurred, view, decorated);
}

/*

    // Antes del retorno temprano: el ancla tiene que estar aunque la página
    // no exista (ver `draw_atlas_anchor`).
    draw_atlas_anchor(scene);
    let Ok(page) = doc.page() else {
        return;
    };
    let page_rect = Rect::new(0.0, 0.0, page.width, page.height);

    // Fondo de la página: color sólido o tablero de transparencia.
    match page.background {
        Some([r, g, b, a]) => {
            scene.fill(
                Fill::NonZero,
                view,
                vello::peniko::Color::from_rgba8(r, g, b, a),
                None,
                &page_rect,
            );
        }
        None if !decorated => {}
        None => {
            // El tablero se dibuja en coordenadas de pantalla (tamaño de
            // celda constante al hacer zoom): rellena el rect de la página
            // proyectado, con la imagen 2x2 repetida y escalada a 8px/celda.
            let brush = vello::peniko::ImageBrush {
                image: checker_image().clone(),
                sampler: vello::peniko::ImageSampler {
                    x_extend: vello::peniko::Extend::Repeat,
                    y_extend: vello::peniko::Extend::Repeat,
                    quality: vello::peniko::ImageQuality::Low,
                    alpha: 1.0,
                },
            };
            scene.fill(
                Fill::NonZero,
                view,
                &brush,
                Some(Affine::scale(8.0)),
                &page_rect,
            );
        }
    }

    // Capas, de abajo arriba, recortadas al rect de la página (lo que
    // sobresale del lienzo no se ve ni se hornea). Se recorre con un índice
    // (no un `for … in`) porque los grupos necesitan abrir/cerrar su propia
    // capa de opacidad alrededor de todo su subárbol contiguo (invariante de
    // preorden de `Page`): `open` lleva, por cada grupo abierto, el índice de
    // su último descendiente y si de verdad se empujó una capa de opacidad
    // (alpha < 1.0 — el camino rápido con alpha == 1.0 no cuesta nada extra).
    // Las hojas hacen lo mismo, más ceñido, alrededor de sombra+contenido.
    // Los `push_layer` anidados se multiplican solos: así `Layer::opacity` se
    // honra de forma uniforme y estructural en las cuatro clases de capa.
    scene.push_layer(
        Fill::NonZero,
        vello::peniko::Mix::Normal,
        1.0,
        view,
        &page_rect,
    );
    // Pila de grupos abiertos: (índice del último descendiente, si se
    // empujó una capa de opacidad). La profundidad de anidamiento real de
    // grupos es casi siempre ≤ 5; pre-reservar 8 evita la primera
    // reallocación (que ocurriría en el 4º `push` con la estrategia de
    // crecimiento por defecto de `Vec`). En el hot path de render esto se
    // llama por cada slot visible por frame.
    let mut open: Vec<(usize, bool)> = Vec::with_capacity(8);
    let mut i = 0usize;
    while i < page.layers.len() {
        while open.last().is_some_and(|&(end, _)| i > end) {
            let (_, pushed) = open.pop().expect("comprobado en la condición del while");
            if pushed {
                scene.pop_layer();
            }
        }
        let layer = &page.layers[i];
        let len = page.subtree_len(i);
        if !layer.visible {
            i += 1 + len; // un grupo oculto se salta entero, con sus hijos
            continue;
        }

        let alpha = layer.opacity.clamp(0.0, 1.0);
        let fade = alpha < 1.0;
        if fade {
            scene.push_layer(
                Fill::NonZero,
                vello::peniko::Mix::Normal,
                alpha,
                view,
                &page_rect,
            );
        }

        if matches!(layer.content, LayerContent::Group(_)) {
            // El grupo no pinta nada por sí mismo: solo deja abierta su capa
            // de opacidad (si la hay) hasta que se recorra todo su subárbol.
            open.push((i + len, fade));
            i += 1;
            continue;
        }

        // Sombra proyectada (rectangular, difusa) por debajo de la capa.
        if let Some(shadow) = layer.effects.shadow {
            let t = layer.transform;
            let rect = Rect::new(
                t.x + shadow.offset_x,
                t.y + shadow.offset_y,
                t.x + t.width + shadow.offset_x,
                t.y + t.height + shadow.offset_y,
            );
            scene.draw_blurred_rounded_rect(
                view,
                rect,
                vello::peniko::Color::BLACK.with_alpha(shadow.opacity.clamp(0.0, 1.0)),
                0.0,
                f64::from(shadow.blur.max(0.0)),
            );
        }
        match &layer.content {
            LayerContent::Image(content) => {
                // `if let`, NO `let … else { continue }`: saltarse el resto del
                // cuerpo del bucle se saltaría también el `i += 1` y el
                // `pop_layer` del final, colgando el hilo de render.
                if let Some(image) = drawable_image(blurred, images, layer.id) {
                    let t = layer.transform;
                    let place = place_transform(&t);

                    // Recorte no destructivo: la fracción `crop` del mapa de bits
                    // llena el rect; el resto se recorta con una capa de clip.
                    let crop = content
                        .crop
                        .map(canvas_core::CropRect::clamped)
                        .unwrap_or_else(canvas_core::CropRect::full);
                    let (iw, ih) = (f64::from(image.width), f64::from(image.height));
                    let sx = t.width / (crop.width * iw);
                    let sy = t.height / (crop.height * ih);
                    let image_local = Affine::scale_non_uniform(sx, sy)
                        * Affine::translate((-crop.x * iw, -crop.y * ih));

                    let cropped = content.crop.is_some();
                    if cropped {
                        scene.push_layer(
                            Fill::NonZero,
                            vello::peniko::Mix::Normal,
                            1.0,
                            view * place,
                            &Rect::new(0.0, 0.0, t.width, t.height),
                        );
                    }
                    scene.draw_image(image, view * place * image_local);
                    if cropped {
                        scene.pop_layer();
                    }
                }
            }
            // El SVG pinta sus píxeles rasterizados del ImageMap (la fuente
            // vectorial viaja en el documento para reexportar sin pérdida).
            LayerContent::Svg(_) => {
                // Mismo motivo que en `Image`: nada de `continue` aquí.
                if let Some(image) = drawable_image(blurred, images, layer.id) {
                    let t = layer.transform;
                    let place = place_transform(&t);
                    let image_local = Affine::scale_non_uniform(
                        t.width / f64::from(image.width),
                        t.height / f64::from(image.height),
                    );
                    scene.draw_image(image, view * place * image_local);
                }
            }
            LayerContent::Text(text) => {
                let t = layer.transform;
                draw_text(scene, view * place_transform(&t), text, t.width);
            }
            LayerContent::Shape(shape) => draw_shape(scene, layer, shape, view),
            // Los grupos se gestionan más arriba (con `continue`, antes de
            // sombra+contenido): esta rama nunca se alcanza para ellos.
            LayerContent::Group(_) => unreachable!("los grupos no llegan a pintarse aquí"),
        }

        if fade {
            scene.pop_layer();
        }
        i += 1;
    }
    // Cierra cualquier grupo que quedara abierto al final de la página (no
    // debería quedar ninguno si la invariante de preorden se cumple, pero un
    // documento restaurado de un sidecar corrupto no puede colgar el hilo de
    // render por un `push_layer`/`pop_layer` desbalanceado).
    while let Some((_, pushed)) = open.pop() {
        if pushed {
            scene.pop_layer();
        }
    }
    debug_assert!(open.is_empty());

    scene.pop_layer();

    // Borde sutil de la página por encima de todo (solo en pantalla).
    if decorated {
        scene.stroke(
            &vello::kurbo::Stroke::new(1.0),
            view,
            palette::css::BLACK.with_alpha(0.25),
            None,
            &page_rect,
        );
    }
}
*/
