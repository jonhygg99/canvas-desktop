//! Alineacion de una transformada dentro de un contenedor, y los dos encajes
//! clasicos de una imagen en la pagina (cubrir y contener).

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

/// Transform que ENCAJA la imagen dentro de la página conservando su
/// proporción (estilo «contain»: escala al mínimo necesario para tocar el
/// borde que antes llegue y centra). A diferencia del encaje al añadir una
/// capa sobre un lienzo no vacío, aquí sí se amplía si la imagen es más
/// pequeña que la página.
pub fn contain_transform(natural_w: f64, natural_h: f64, page_w: f64, page_h: f64) -> Transform {
    if natural_w <= 0.0 || natural_h <= 0.0 {
        return Transform::new(0.0, 0.0, page_w.max(1.0), page_h.max(1.0));
    }
    let scale = (page_w / natural_w).min(page_h / natural_h);
    let width = natural_w * scale;
    let height = natural_h * scale;
    Transform::new(
        (page_w - width) / 2.0,
        (page_h - height) / 2.0,
        width,
        height,
    )
}
