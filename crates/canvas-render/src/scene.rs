//! Construcción de la escena vello a partir del documento.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use canvas_core::{Document, LayerContent, LayerId};
use vello::kurbo::{Affine, Rect};
use vello::peniko::color::palette;
use vello::peniko::{Blob, Fill, ImageData};
use vello::Scene;

/// Mapa de bits de cada capa de imagen, gestionado por la app.
pub type ImageMap = HashMap<LayerId, ImageData>;

/// Empaqueta un buffer RGBA8 como imagen de vello.
pub fn image_data_from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> ImageData {
    ImageData {
        data: Blob::new(Arc::new(rgba)),
        format: vello::peniko::ImageFormat::Rgba8,
        alpha_type: vello::peniko::ImageAlphaType::Alpha,
        width,
        height,
    }
}

/// Tablero de ajedrez 2x2 (gris/blanco) que se repite bajo la página para
/// hacer visible la transparencia.
fn checker_image() -> &'static ImageData {
    static CHECKER: OnceLock<ImageData> = OnceLock::new();
    CHECKER.get_or_init(|| {
        const LIGHT: [u8; 4] = [252, 252, 252, 255];
        const DARK: [u8; 4] = [222, 222, 222, 255];
        let mut rgba = Vec::with_capacity(2 * 2 * 4);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            rgba.extend_from_slice(if (x + y) % 2 == 0 { &LIGHT } else { &DARK });
        }
        image_data_from_rgba(rgba, 2, 2)
    })
}

/// Construye la escena del documento con la transformación de vista dada
/// (página → píxeles físicos del lienzo). `blurred` sustituye la imagen de
/// las capas con desenfoque activo (textura GPU ya procesada).
///
/// Con `decorated` se pintan los adornos de edición (tablero de transparencia
/// y borde de página); el horneado para guardar/exportar va sin ellos.
pub fn build_scene(
    doc: &Document,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    decorated: bool,
) -> Scene {
    let mut scene = Scene::new();
    let Ok(page) = doc.page() else {
        return scene;
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

    // Capas, de abajo arriba.
    for layer in &page.layers {
        if !layer.visible {
            continue;
        }
        match &layer.content {
            LayerContent::Image(_) => {
                let Some(image) = blurred.get(&layer.id).or_else(|| images.get(&layer.id)) else {
                    continue;
                };
                if image.width == 0 || image.height == 0 {
                    continue;
                }
                let t = layer.transform;
                let local = Affine::translate((t.x, t.y))
                    * Affine::rotate_about(
                        t.rotation.to_radians(),
                        vello::kurbo::Point::new(t.width / 2.0, t.height / 2.0),
                    )
                    * Affine::scale_non_uniform(
                        t.width / f64::from(image.width),
                        t.height / f64::from(image.height),
                    );
                scene.draw_image(image, view * local);
            }
        }
    }

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

    scene
}
