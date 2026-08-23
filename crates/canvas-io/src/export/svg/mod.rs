//! Generacion del SVG a partir del documento.
//!
//! Recorre `page.layers` con el MISMO indice + pila de grupos abiertos que
//! `canvas_render::scene::build_scene`, para que la jerarquia de
//! opacidad/grupo sea identica. Los efectos (desenfoque, ajustes de color) NO
//! necesitan filtros SVG: el llamador entrega en `images` los pixeles ya
//! procesados (los mismos que ve el lienzo).

use base64::Engine;
use canvas_core::{Document, LayerContent};

use crate::IoError;

use super::{export_error, ExportImages, TextLineBreaker};

mod image;
mod shape;
mod text;
mod util;

use image::image_element;
use shape::shape_element;
use text::text_element;
use util::{alpha, hex, n, place_transform_svg};

/// Genera el SVG completo del documento. `scale` cambia el tamaño
/// DECLARADO del `<svg>` (lo que significa "2x" en un formato vectorial), no
/// el `viewBox`, que sigue en coordenadas de página.
pub fn document_to_svg(
    doc: &Document,
    images: &ExportImages,
    scale: f64,
    text_lines: &TextLineBreaker<'_>,
) -> Result<String, IoError> {
    let page = doc
        .page()
        .map_err(|e| export_error(format!("document has no page: {e}")))?;
    let (w, h) = (page.width, page.height);

    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        n(w * scale),
        n(h * scale),
        n(w),
        n(h),
    ));
    svg.push_str(&format!(
        "<defs><clipPath id=\"page\"><rect width=\"{}\" height=\"{}\"/></clipPath></defs>\n",
        n(w),
        n(h),
    ));
    if let Some(bg) = page.background {
        svg.push_str(&format!(
            "<rect width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"{}\"/>\n",
            n(w),
            n(h),
            hex(bg),
            n(alpha(bg)),
        ));
    }
    svg.push_str("<g clip-path=\"url(#page)\">\n");

    // Mismo recorrido (índice + pila de fin de subárbol) que
    // `canvas_render::scene::build_scene`, para que la jerarquía de
    // grupos/opacidad del SVG sea idéntica a la del lienzo.
    let mut group_ends: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < page.layers.len() {
        while group_ends.last().is_some_and(|&end| i > end) {
            group_ends.pop();
            svg.push_str("</g>\n");
        }
        let layer = &page.layers[i];
        let len = page.subtree_len(i);
        if !layer.visible {
            i += 1 + len; // un grupo oculto se salta entero, con sus hijos
            continue;
        }
        let layer_alpha = f64::from(layer.opacity.clamp(0.0, 1.0));

        if matches!(layer.content, LayerContent::Group(_)) {
            svg.push_str(&format!("<g opacity=\"{}\">\n", n(layer_alpha)));
            group_ends.push(i + len);
            i += 1;
            continue;
        }

        svg.push_str(&format!("<g opacity=\"{}\">\n", n(layer_alpha)));
        let t = layer.transform;

        // Sombra proyectada: en coordenadas de PÁGINA, sin `place_transform`
        // (igual que `draw_blurred_rounded_rect` en el renderer).
        if let Some(shadow) = layer.effects.shadow {
            let filter_id = format!("shadow{}", layer.id.raw());
            svg.push_str(&format!(
                "<filter id=\"{fid}\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\"><feGaussianBlur stdDeviation=\"{blur}\"/></filter>\n",
                fid = filter_id,
                blur = n(f64::from(shadow.blur.max(0.0))),
            ));
            svg.push_str(&format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"#000000\" fill-opacity=\"{op}\" filter=\"url(#{fid})\"/>\n",
                x = n(t.x + shadow.offset_x),
                y = n(t.y + shadow.offset_y),
                w = n(t.width),
                h = n(t.height),
                op = n(f64::from(shadow.opacity.clamp(0.0, 1.0))),
                fid = filter_id,
            ));
        }

        svg.push_str(&format!("<g transform=\"{}\">\n", place_transform_svg(&t)));
        match &layer.content {
            LayerContent::Image(content) => {
                image_element(
                    &mut svg,
                    images.get(&layer.id.raw()),
                    &t,
                    content.crop,
                    content.natural_width,
                    content.natural_height,
                );
            }
            LayerContent::Svg(content) => {
                if content.source.is_empty() {
                    image_element(
                        &mut svg,
                        images.get(&layer.id.raw()),
                        &t,
                        None,
                        content.natural_width,
                        content.natural_height,
                    );
                } else {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&content.source);
                    svg.push_str(&format!(
                        "<image x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" xlink:href=\"data:image/svg+xml;base64,{b64}\"/>\n",
                        w = n(t.width),
                        h = n(t.height),
                    ));
                }
            }
            LayerContent::Text(text) => {
                text_element(&mut svg, text, t.width, text_lines);
            }
            LayerContent::Shape(shape) => {
                shape_element(&mut svg, shape, t.width, t.height);
            }
            LayerContent::Group(_) => unreachable!("los grupos se gestionan más arriba"),
        }
        svg.push_str("</g>\n"); // cierra <g transform> (place_transform)
        svg.push_str("</g>\n"); // cierra <g opacity> de la hoja
        i += 1;
    }
    while group_ends.pop().is_some() {
        svg.push_str("</g>\n");
    }
    svg.push_str("</g>\n"); // cierra el clip de página
    svg.push_str("</svg>\n");
    Ok(svg)
}
