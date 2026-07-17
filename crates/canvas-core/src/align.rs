//! Geometría pura de alineación y redimensionado. Sin UI: funciones
//! deterministas y testeables que la app usa para botones y manejadores.

use crate::layer::Transform;

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
        rotation: start.rotation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(x: f64, y: f64, w: f64, h: f64) -> Transform {
        Transform::new(x, y, w, h)
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
    fn resize_clamps_to_min_size_without_flipping() {
        let start = t(0.0, 0.0, 100.0, 100.0);
        let r = resize_from_corner(&start, Corner::BottomRight, -500.0, -500.0, false, 8.0);
        assert_eq!((r.width, r.height), (8.0, 8.0));

        let locked = resize_from_corner(&start, Corner::BottomRight, -500.0, -500.0, true, 8.0);
        assert!(locked.width >= 8.0 && locked.height >= 8.0);
        assert!((locked.aspect_ratio() - 1.0).abs() < 1e-9);
    }
}
