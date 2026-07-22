//! Geometría pura de alineación y redimensionado. Sin UI: funciones
//! deterministas y testeables que la app usa para botones y manejadores.

use crate::layer::{CropRect, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

/// Alinea horizontalmente respecto a un contenedor de ancho `container_width`
/// (la página, o el cuadro que engloba una selección múltiple).
pub fn align_horizontal(t: &Transform, container_width: f64, align: HAlign) -> Transform {
    let x = match align {
        HAlign::Left => 0.0,
        HAlign::Center => (container_width - t.width) / 2.0,
        HAlign::Right => container_width - t.width,
    };
    Transform { x, ..*t }
}

/// Alinea verticalmente respecto a un contenedor de alto `container_height`.
pub fn align_vertical(t: &Transform, container_height: f64, align: VAlign) -> Transform {
    let y = match align {
        VAlign::Top => 0.0,
        VAlign::Middle => (container_height - t.height) / 2.0,
        VAlign::Bottom => container_height - t.height,
    };
    Transform { y, ..*t }
}

/// Transform que hace que una imagen CUBRA la página entera conservando su
/// proporción (estilo «cover»: escala al máximo necesario y centra; lo que
/// sobresale se recorta al renderizar).
pub fn cover_transform(natural_w: f64, natural_h: f64, page_w: f64, page_h: f64) -> Transform {
    if natural_w <= 0.0 || natural_h <= 0.0 {
        return Transform::new(0.0, 0.0, page_w.max(1.0), page_h.max(1.0));
    }
    let scale = (page_w / natural_w).max(page_h / natural_h);
    let width = natural_w * scale;
    let height = natural_h * scale;
    Transform::new(
        (page_w - width) / 2.0,
        (page_h - height) / 2.0,
        width,
        height,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Redimensiona arrastrando la esquina `corner`; la esquina opuesta queda
/// anclada. `dx`/`dy` es el desplazamiento del puntero en coordenadas de
/// página desde el inicio del gesto (`start`).
///
/// Con `keep_aspect` la relación de aspecto de `start` se mantiene usando el
/// eje con mayor cambio relativo. El tamaño nunca baja de `min_size` en
/// ninguno de los dos ejes (sin volteo al cruzar el ancla).
pub fn resize_from_corner(
    start: &Transform,
    corner: Corner,
    dx: f64,
    dy: f64,
    keep_aspect: bool,
    min_size: f64,
) -> Transform {
    let min_size = min_size.max(1.0);
    if start.width <= 0.0 || start.height <= 0.0 {
        return *start;
    }

    // Deltas de tamaño según qué esquina se arrastra (crecer = alejarse del ancla).
    let (dw, dh) = match corner {
        Corner::TopLeft => (-dx, -dy),
        Corner::TopRight => (dx, -dy),
        Corner::BottomLeft => (-dx, dy),
        Corner::BottomRight => (dx, dy),
    };

    let mut width = (start.width + dw).max(min_size);
    let mut height = (start.height + dh).max(min_size);

    if keep_aspect {
        let sx = width / start.width;
        let sy = height / start.height;
        // El eje con mayor cambio relativo manda.
        let s = if (sx - 1.0).abs() >= (sy - 1.0).abs() {
            sx
        } else {
            sy
        };
        width = start.width * s;
        height = start.height * s;
        // Reimpone el mínimo sin romper la proporción.
        if width < min_size || height < min_size {
            let s_min = (min_size / start.width).max(min_size / start.height);
            width = start.width * s_min;
            height = start.height * s_min;
        }
    }

    // Recoloca para que la esquina opuesta (el ancla) no se mueva.
    let (x, y) = match corner {
        Corner::TopLeft => (
            start.x + start.width - width,
            start.y + start.height - height,
        ),
        Corner::TopRight => (start.x, start.y + start.height - height),
        Corner::BottomLeft => (start.x + start.width - width, start.y),
        Corner::BottomRight => (start.x, start.y),
    };

    Transform {
        x,
        y,
        width,
        height,
        ..*start
    }
}

/// Como [`resize_from_corner`], pero correcto con la capa ROTADA: el delta
/// del puntero llega en coordenadas de página, se pasa al espacio local de la
/// capa, se redimensiona ahí, y la esquina opuesta (el ancla) queda clavada
/// EN PÁGINA aunque el rect gire alrededor de su centro.
pub fn resize_rotated_from_corner(
    start: &Transform,
    corner: Corner,
    page_dx: f64,
    page_dy: f64,
    keep_aspect: bool,
    min_size: f64,
) -> Transform {
    let theta = start.rotation.to_radians();
    if theta == 0.0 {
        return resize_from_corner(start, corner, page_dx, page_dy, keep_aspect, min_size);
    }
    // Delta del puntero en el espacio local (des-rotado).
    let (sin, cos) = (-theta).sin_cos();
    let local_dx = page_dx * cos - page_dy * sin;
    let local_dy = page_dx * sin + page_dy * cos;

    // Redimensiona en local: solo interesan width/height nuevos.
    let resized = resize_from_corner(start, corner, local_dx, local_dy, keep_aspect, min_size);
    let (w, h) = (resized.width, resized.height);

    // Ancla: la esquina opuesta, en coordenadas de página (rotada).
    let anchor_index = match corner {
        Corner::TopLeft => 3,     // ancla = inferior derecha
        Corner::TopRight => 2,    // ancla = inferior izquierda
        Corner::BottomLeft => 1,  // ancla = superior derecha
        Corner::BottomRight => 0, // ancla = superior izquierda
    };
    let anchor = start.corners()[anchor_index];

    // Vector local del ancla al centro con las dimensiones nuevas.
    let (ox, oy) = match corner {
        Corner::TopLeft => (-w / 2.0, -h / 2.0),
        Corner::TopRight => (w / 2.0, -h / 2.0),
        Corner::BottomLeft => (-w / 2.0, h / 2.0),
        Corner::BottomRight => (w / 2.0, h / 2.0),
    };
    // Centro nuevo = ancla + R(θ)·(vector local ancla→centro).
    let (sin_f, cos_f) = theta.sin_cos();
    let cx = anchor.0 + (ox * cos_f - oy * sin_f);
    let cy = anchor.1 + (ox * sin_f + oy * cos_f);

    Transform {
        x: cx - w / 2.0,
        y: cy - h / 2.0,
        width: w,
        height: h,
        ..*start
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn t(x: f64, y: f64, w: f64, h: f64) -> Transform {
        Transform::new(x, y, w, h)
    }

    #[test]
    fn uncrop_restores_full_content_in_place() {
        // Recorte del cuarto superior izquierdo de una imagen 100×100 en (25,10).
        let start = t(0.0, 0.0, 100.0, 100.0);
        let (cropped_t, crop) =
            trim_crop_from_corner(&start, CropRect::full(), Corner::BottomRight, -50.0, -50.0);
        let restored = uncrop_transform(&cropped_t, crop);
        assert!((restored.x - start.x).abs() < 1e-9);
        assert!((restored.y - start.y).abs() < 1e-9);
        assert!((restored.width - 100.0).abs() < 1e-9);
        assert!((restored.height - 100.0).abs() < 1e-9);
    }

    #[test]
    fn horizontal_alignment_against_page() {
        let layer = t(37.0, 50.0, 200.0, 100.0);
        assert_eq!(align_horizontal(&layer, 800.0, HAlign::Left).x, 0.0);
        assert_eq!(align_horizontal(&layer, 800.0, HAlign::Center).x, 300.0);
        assert_eq!(align_horizontal(&layer, 800.0, HAlign::Right).x, 600.0);
        // La Y no cambia al alinear en horizontal.
        assert_eq!(align_horizontal(&layer, 800.0, HAlign::Center).y, 50.0);
    }

    #[test]
    fn vertical_alignment_against_page() {
        let layer = t(37.0, 50.0, 200.0, 100.0);
        assert_eq!(align_vertical(&layer, 600.0, VAlign::Top).y, 0.0);
        assert_eq!(align_vertical(&layer, 600.0, VAlign::Middle).y, 250.0);
        assert_eq!(align_vertical(&layer, 600.0, VAlign::Bottom).y, 500.0);
        assert_eq!(align_vertical(&layer, 600.0, VAlign::Middle).x, 37.0);
    }

    #[test]
    fn cover_scales_up_and_centers() {
        // Imagen 4:3 sobre página 16:9: manda el ancho, sobra alto.
        let c = cover_transform(800.0, 600.0, 1920.0, 1080.0);
        assert_eq!((c.width, c.height), (1920.0, 1440.0));
        assert_eq!(c.x, 0.0);
        assert_eq!(c.y, (1080.0 - 1440.0) / 2.0);

        // Imagen apaisada sobre página vertical: manda el alto.
        let c = cover_transform(1920.0, 1080.0, 1080.0, 1920.0);
        assert!((c.height - 1920.0).abs() < 1e-9);
        assert!(c.width > 1080.0);
        assert!((c.x - (1080.0 - c.width) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn resize_bottom_right_keeps_top_left_anchored() {
        let start = t(10.0, 20.0, 100.0, 50.0);
        let r = resize_from_corner(&start, Corner::BottomRight, 50.0, 25.0, false, 1.0);
        assert_eq!((r.x, r.y), (10.0, 20.0));
        assert_eq!((r.width, r.height), (150.0, 75.0));
    }

    #[test]
    fn resize_top_left_keeps_bottom_right_anchored() {
        let start = t(10.0, 20.0, 100.0, 50.0);
        let r = resize_from_corner(&start, Corner::TopLeft, -20.0, -10.0, false, 1.0);
        assert_eq!((r.width, r.height), (120.0, 60.0));
        // La esquina inferior derecha (110, 70) no se mueve.
        assert_eq!((r.x + r.width, r.y + r.height), (110.0, 70.0));
        assert_eq!((r.x, r.y), (-10.0, 10.0));
    }

    #[test]
    fn aspect_lock_preserves_ratio_using_dominant_axis() {
        let start = t(0.0, 0.0, 200.0, 100.0);
        // dx domina (50% de cambio frente a 10%).
        let r = resize_from_corner(&start, Corner::BottomRight, 100.0, 10.0, true, 1.0);
        assert_eq!((r.width, r.height), (300.0, 150.0));
        assert!((r.aspect_ratio() - start.aspect_ratio()).abs() < 1e-9);
    }

    #[test]
    fn aspect_lock_shrinks_too() {
        let start = t(0.0, 0.0, 200.0, 100.0);
        let r = resize_from_corner(&start, Corner::BottomRight, -100.0, -10.0, true, 1.0);
        assert_eq!((r.width, r.height), (100.0, 50.0));
    }

    #[test]
    fn unlocked_resize_changes_ratio() {
        let start = t(0.0, 0.0, 200.0, 100.0);
        let r = resize_from_corner(&start, Corner::BottomRight, 0.0, 100.0, false, 1.0);
        assert_eq!((r.width, r.height), (200.0, 200.0));
    }

    #[test]
    fn rotated_resize_keeps_opposite_corner_anchored() {
        let mut start = t(100.0, 100.0, 200.0, 100.0);
        start.rotation = 30.0;
        let anchor_before = start.corners()[0]; // superior izquierda

        // Arrastra la esquina inferior derecha 40 px en página.
        let r = resize_rotated_from_corner(&start, Corner::BottomRight, 40.0, 10.0, false, 1.0);
        let anchor_after = r.corners()[0];
        assert!((anchor_after.0 - anchor_before.0).abs() < 1e-9);
        assert!((anchor_after.1 - anchor_before.1).abs() < 1e-9);
        assert_eq!(r.rotation, 30.0);
    }

    #[test]
    fn rotated_resize_with_zero_rotation_matches_plain() {
        let start = t(10.0, 20.0, 100.0, 50.0);
        let plain = resize_from_corner(&start, Corner::BottomRight, 30.0, 15.0, true, 1.0);
        let rotated =
            resize_rotated_from_corner(&start, Corner::BottomRight, 30.0, 15.0, true, 1.0);
        assert_eq!(plain, rotated);
    }

    #[test]
    fn trim_crop_shrinks_window_and_keeps_content_fixed() {
        let start = t(0.0, 0.0, 100.0, 100.0);
        let (nt, crop) =
            trim_crop_from_corner(&start, CropRect::full(), Corner::BottomRight, -20.0, -30.0);
        assert_eq!((nt.x, nt.y), (0.0, 0.0)); // la esquina opuesta no se mueve
        assert_eq!((nt.width, nt.height), (80.0, 70.0));
        assert!((crop.width - 0.8).abs() < 1e-9);
        assert!((crop.height - 0.7).abs() < 1e-9);
        assert_eq!((crop.x, crop.y), (0.0, 0.0));
    }

    #[test]
    fn trim_crop_cannot_expand_beyond_content() {
        let start = t(0.0, 0.0, 100.0, 100.0);
        // Sin recorte previo no hay contenido extra: expandir no hace nada.
        let (nt, crop) =
            trim_crop_from_corner(&start, CropRect::full(), Corner::BottomRight, 50.0, 50.0);
        assert_eq!((nt.width, nt.height), (100.0, 100.0));
        assert_eq!(crop, CropRect::full().clamped());
    }

    #[test]
    fn trim_crop_top_left_moves_origin_and_crop_offset() {
        let start = t(0.0, 0.0, 100.0, 100.0);
        let (nt, crop) =
            trim_crop_from_corner(&start, CropRect::full(), Corner::TopLeft, 25.0, 10.0);
        // El borde izquierdo entra 25 px y el superior 10.
        assert_eq!((nt.x, nt.y), (25.0, 10.0));
        assert_eq!((nt.width, nt.height), (75.0, 90.0));
        assert!((crop.x - 0.25).abs() < 1e-9);
        assert!((crop.y - 0.10).abs() < 1e-9);
        // Deshacer el recorte (expandir de nuevo) recupera contenido.
        let (nt2, crop2) = trim_crop_from_corner(&nt, crop, Corner::TopLeft, -25.0, -10.0);
        assert!((nt2.x).abs() < 1e-9);
        assert!((crop2.x).abs() < 1e-9);
        assert!((crop2.width - 1.0).abs() < 1e-9);
    }

    #[test]
    fn contains_point_respects_rotation() {
        let mut layer = t(0.0, 0.0, 100.0, 20.0);
        // Punto justo fuera de la esquina AABB.
        assert!(!layer.contains_point(95.0, 25.0));
        layer.rotation = 90.0; // ahora es alto y estrecho alrededor de (50,10)
        assert!(layer.contains_point(50.0, 55.0));
        assert!(!layer.contains_point(95.0, 10.0));
    }

    #[test]
    fn resize_clamps_to_min_size_without_flipping() {
        let start = t(0.0, 0.0, 100.0, 100.0);
        let r = resize_from_corner(&start, Corner::BottomRight, -500.0, -500.0, false, 8.0);
        assert_eq!((r.width, r.height), (8.0, 8.0));

        let locked = resize_from_corner(&start, Corner::BottomRight, -500.0, -500.0, true, 8.0);
        assert!(locked.width >= 8.0 && locked.height >= 8.0);
        assert!((locked.aspect_ratio() - 1.0).abs() < 1e-9);
    }
}
