//! Mapas de bits: la matriz de colocacion de una capa, el envoltorio de
//! pixeles RGBA que consume vello, y el tablero de transparencia del fondo.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use canvas_core::LayerId;
use vello::kurbo::Affine;
use vello::peniko::{Blob, ImageData};

/// Colocación del rect de una capa: posición + rotación sobre el centro +
/// volteo sobre el centro.
pub(super) fn place_transform(t: &canvas_core::Transform) -> Affine {
    let center = vello::kurbo::Point::new(t.width / 2.0, t.height / 2.0);
    let flip = Affine::translate((center.x, center.y))
        * Affine::scale_non_uniform(
            if t.flip_h { -1.0 } else { 1.0 },
            if t.flip_v { -1.0 } else { 1.0 },
        )
        * Affine::translate((-center.x, -center.y));
    Affine::translate((t.x, t.y)) * Affine::rotate_about(t.rotation.to_radians(), center) * flip
}

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
pub(super) fn checker_image() -> &'static ImageData {
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
