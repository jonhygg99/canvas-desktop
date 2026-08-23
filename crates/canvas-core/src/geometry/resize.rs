//! Redimensionado: alrededor del centro, o arrastrando una esquina (con y sin
//! rotacion, anclando siempre la esquina opuesta).

use crate::layer::Transform;

/// Cambia el tamaño de una capa manteniendo su centro fijo (el aumento o la
/// disminución se reparte por igual a los cuatro lados). Es lo que usan los
/// campos de tamaño del panel de propiedades; los tiradores de esquina del
/// lienzo, en cambio, anclan la esquina opuesta (`resize_from_corner`).
///
/// La rotación no necesita tratamiento especial: ya es alrededor del centro.
pub fn resize_around_center(start: &Transform, width: f64, height: f64) -> Transform {
    let (cx, cy) = start.center();
    let (width, height) = (width.max(1.0), height.max(1.0));
    Transform {
        x: cx - width / 2.0,
        y: cy - height / 2.0,
        width,
        height,
        ..*start
    }
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
