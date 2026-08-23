//! Recorte «no destructivo»: recortar por los bordes deja el contenido quieto
//! y encoge la ventana; deshacerlo devuelve el contenido completo en su sitio.

use crate::layer::{CropRect, Transform};

use super::Corner;

/// Recorte «por bordes» arrastrando una esquina en modo recorte: la esquina
/// mueve los dos bordes adyacentes; el CONTENIDO queda clavado en la página
/// (la ventana visible se estrecha o se ensancha sobre él) y el rect de la
/// capa se ajusta en consecuencia. Devuelve el transform y el crop nuevos.
///
/// La expansión se limita a lo que quede de imagen fuera del recorte actual,
/// y la reducción a un mínimo de 8 px de página por eje.
pub fn trim_crop_from_corner(
    start: &Transform,
    start_crop: CropRect,
    corner: Corner,
    page_dx: f64,
    page_dy: f64,
) -> (Transform, CropRect) {
    const MIN_PX: f64 = 8.0;
    let start_crop = start_crop.clamped();
    let theta = start.rotation.to_radians();

    // Delta del puntero en el espacio local de la capa.
    let (sin_inv, cos_inv) = (-theta).sin_cos();
    let local_dx = page_dx * cos_inv - page_dy * sin_inv;
    let local_dy = page_dx * sin_inv + page_dy * cos_inv;

    // Tamaño del mapa de bits COMPLETO en píxeles de página.
    let full_w = start.width / start_crop.width;
    let full_h = start.height / start_crop.height;
    // Márgenes de contenido disponibles para expandir por cada lado.
    let max_left = start_crop.x * full_w;
    let max_right = (1.0 - start_crop.x - start_crop.width) * full_w;
    let max_top = start_crop.y * full_h;
    let max_bottom = (1.0 - start_crop.y - start_crop.height) * full_h;

    // Cambio de cada borde en local (positivo = expandir hacia fuera).
    let (mut d_left, mut d_right, mut d_top, mut d_bottom) = (0.0, 0.0, 0.0, 0.0);
    match corner {
        Corner::TopLeft => {
            d_left = -local_dx;
            d_top = -local_dy;
        }
        Corner::TopRight => {
            d_right = local_dx;
            d_top = -local_dy;
        }
        Corner::BottomLeft => {
            d_left = -local_dx;
            d_bottom = local_dy;
        }
        Corner::BottomRight => {
            d_right = local_dx;
            d_bottom = local_dy;
        }
    }
    let shrink_w = start.width - MIN_PX;
    let shrink_h = start.height - MIN_PX;
    d_left = d_left.clamp(-shrink_w, max_left);
    d_right = d_right.clamp(-shrink_w, max_right);
    d_top = d_top.clamp(-shrink_h, max_top);
    d_bottom = d_bottom.clamp(-shrink_h, max_bottom);

    let new_w = (start.width + d_left + d_right).max(MIN_PX);
    let new_h = (start.height + d_top + d_bottom).max(MIN_PX);

    let crop = CropRect {
        x: start_crop.x - d_left / full_w,
        y: start_crop.y - d_top / full_h,
        width: new_w / full_w,
        height: new_h / full_h,
    }
    .clamped();

    // El centro local se desplaza la mitad de lo que cambian los bordes
    // opuestos; a página con la rotación de la capa.
    let (shift_x, shift_y) = ((d_right - d_left) / 2.0, (d_bottom - d_top) / 2.0);
    let (sin_f, cos_f) = theta.sin_cos();
    let (pcx, pcy) = start.center();
    let cx = pcx + shift_x * cos_f - shift_y * sin_f;
    let cy = pcy + shift_x * sin_f + shift_y * cos_f;

    (
        Transform {
            x: cx - new_w / 2.0,
            y: cy - new_h / 2.0,
            width: new_w,
            height: new_h,
            ..*start
        },
        crop,
    )
}

/// Transform que muestra la imagen COMPLETA de nuevo (quitar el recorte),
/// dejando el contenido clavado en la página.
pub fn uncrop_transform(t: &Transform, crop: CropRect) -> Transform {
    let crop = crop.clamped();
    let full_w = t.width / crop.width;
    let full_h = t.height / crop.height;
    let d_left = crop.x * full_w;
    let d_right = (1.0 - crop.x - crop.width) * full_w;
    let d_top = crop.y * full_h;
    let d_bottom = (1.0 - crop.y - crop.height) * full_h;

    let (shift_x, shift_y) = ((d_right - d_left) / 2.0, (d_bottom - d_top) / 2.0);
    let theta = t.rotation.to_radians();
    let (sin_f, cos_f) = theta.sin_cos();
    let (pcx, pcy) = t.center();
    let cx = pcx + shift_x * cos_f - shift_y * sin_f;
    let cy = pcy + shift_x * sin_f + shift_y * cos_f;

    Transform {
        x: cx - full_w / 2.0,
        y: cy - full_h / 2.0,
        width: full_w,
        height: full_h,
        ..*t
    }
}
