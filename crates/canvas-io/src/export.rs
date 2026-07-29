//! Exportación a PNG/JPEG/SVG/PDF. PNG/JPEG reutilizan `bake_page` de
//! canvas-render (el llamador se encarga de hornear y llamar a `save_rgba`);
//! este módulo solo cubre el camino vectorial: generar el SVG a mano a
//! partir del documento, y convertirlo a PDF con `svg2pdf`.
//!
//! El SVG recorre `page.layers` con el MISMO índice + pila de grupos
//! abiertos que `canvas_render::scene::build_scene`, para que la jerarquía
//! de opacidad/grupo sea idéntica. Los efectos (desenfoque, ajustes de
//! color) NO necesitan filtros SVG: el llamador entrega en `images` los
//! píxeles ya procesados (los mismos que ve el lienzo), así que solo hay que
//! basarlos en base64.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use canvas_core::{CropRect, Document, LayerContent, ShapeKind, TextContent, TextLine, Transform};

use crate::IoError;

/// Formato de exportación elegido en el diálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Svg,
    Pdf,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Svg => "SVG",
            Self::Pdf => "PDF",
        }
    }

    /// ¿Necesita el horneado de la GPU (`bake_page`)? PNG/JPEG sí; SVG/PDF
    /// se generan a mano a partir del documento.
    pub fn needs_bake(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg)
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "svg" => Some(Self::Svg),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }
}

/// PNG en base64 (mismo formato que produce `encode_layer_png`) de cada capa
/// raster (Image/Svg), por id crudo de capa. El llamador ya debe haber
/// aplicado desenfoque/ajustes de color antes de codificar: aquí no se
/// reprocesa nada.
pub type ExportImages = HashMap<u64, String>;

/// Resuelve el salto de línea de un texto: lo implementa `canvas-render`
/// con parley (el mismo layout que ve el lienzo), inyectado así para que
/// este crate no dependa de un motor de texto.
pub type TextLineBreaker<'a> = dyn Fn(&TextContent, f64) -> Vec<TextLine> + 'a;

fn export_error(message: impl Into<String>) -> IoError {
    IoError::Encode {
        path: PathBuf::from("export"),
        message: message.into(),
    }
}

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

/// Convierte un SVG (típicamente el que genera `document_to_svg`) a PDF de
/// una página, sin rasterizar texto ni formas.
pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, IoError> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| {
        export_error(format!(
            "the generated SVG could not be parsed for PDF conversion: {e}"
        ))
    })?;
    svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions::default(),
    )
    .map_err(|e| export_error(format!("SVG to PDF conversion failed: {e:?}")))
}

/// `place_transform` (canvas-render/scene.rs) equivalente en SVG. Deriva de
/// `Affine::translate((t.x,t.y)) * Affine::rotate_about(rot, center) *
/// Affine::translate(center) * Affine::scale_non_uniform(sx,sy) *
/// Affine::translate(-center)`; como `rotate_about(th, c) = translate(c) *
/// rotate(th) * translate(-c)` en kurbo, el `translate(-center)` interno se
/// cancela con el `translate(center)` del volteo, quedando exactamente la
/// cadena de abajo (verificado contra la fuente de kurbo 0.11).
fn place_transform_svg(t: &Transform) -> String {
    let (cx, cy) = (t.width / 2.0, t.height / 2.0);
    let sx = if t.flip_h { -1.0 } else { 1.0 };
    let sy = if t.flip_v { -1.0 } else { 1.0 };
    format!(
        "translate({tx} {ty}) translate({cx} {cy}) rotate({rot}) scale({sx} {sy}) translate({ncx} {ncy})",
        tx = n(t.x),
        ty = n(t.y),
        cx = n(cx),
        cy = n(cy),
        rot = n(t.rotation),
        sx = n(sx),
        sy = n(sy),
        ncx = n(-cx),
        ncy = n(-cy),
    )
}

/// `<image>` con el mismo recorte no destructivo que `scene.rs` (espeja
/// `image_local = scale_non_uniform(sx,sy) * translate(-crop.x*iw,
/// -crop.y*ih)`); si no hay píxeles para la capa (no debería pasar: el
/// llamador siempre los entrega), no escribe nada.
fn image_element(
    svg: &mut String,
    png_base64: Option<&String>,
    t: &Transform,
    crop: Option<CropRect>,
    natural_width: u32,
    natural_height: u32,
) {
    let Some(b64) = png_base64 else { return };
    let (iw, ih) = (f64::from(natural_width), f64::from(natural_height));
    if iw <= 0.0 || ih <= 0.0 {
        return;
    }
    let resolved = crop.map(CropRect::clamped).unwrap_or_else(CropRect::full);
    let sx = t.width / (resolved.width * iw);
    let sy = t.height / (resolved.height * ih);
    let (x, y) = (-resolved.x * iw * sx, -resolved.y * ih * sy);
    let (w, h) = (iw * sx, ih * sy);

    if crop.is_some() {
        svg.push_str(&format!(
            "<clipPath id=\"cropN\"><rect width=\"{}\" height=\"{}\"/></clipPath>\n",
            n(t.width),
            n(t.height),
        ));
        svg.push_str("<g clip-path=\"url(#cropN)\">\n");
    }
    svg.push_str(&format!(
        "<image x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" xlink:href=\"data:image/png;base64,{b64}\"/>\n",
        x = n(x),
        y = n(y),
        w = n(w),
        h = n(h),
    ));
    if crop.is_some() {
        svg.push_str("</g>\n");
    }
}

/// `<text>` con un `<tspan x y>` por línea (`text_lines`, las MISMAS
/// métricas de parley que ve el lienzo): así el SVG no toma ninguna
/// decisión de salto de línea ni de alineación por su cuenta.
fn text_element(
    svg: &mut String,
    content: &TextContent,
    box_width: f64,
    text_lines: &TextLineBreaker<'_>,
) {
    let lines = text_lines(content, box_width);
    if lines.is_empty() {
        return;
    }
    let family = if content.family.is_empty() {
        "sans-serif".to_owned()
    } else {
        esc(&content.family)
    };
    svg.push_str(&format!(
        "<text font-family=\"{family}\" font-size=\"{size}\" font-weight=\"{weight}\"{italic} letter-spacing=\"{ls}\" fill=\"{fill}\" fill-opacity=\"{fa}\" xml:space=\"preserve\">\n",
        family = family,
        size = n(f64::from(content.size)),
        weight = content.weight,
        italic = if content.italic { " font-style=\"italic\"" } else { "" },
        ls = n(f64::from(content.letter_spacing)),
        fill = hex(content.color),
        fa = n(alpha(content.color)),
    ));
    for line in lines {
        svg.push_str(&format!(
            "<tspan x=\"{x}\" y=\"{y}\">{text}</tspan>\n",
            x = n(line.x),
            y = n(line.baseline),
            text = esc(&line.text),
        ));
    }
    svg.push_str("</text>\n");
}

/// Rect/ellipse/line nativos de SVG, en la MISMA caja local (0,0)..(w,h) que
/// usa `scene.rs` (mismo criterio para omitir relleno/borde: `if fa > 0` /
/// `if sa > 0 && stroke_width > 0`, y la línea usa el borde si lo hay, si no
/// el relleno).
fn shape_element(svg: &mut String, shape: &canvas_core::ShapeContent, w: f64, h: f64) {
    let [_, _, _, fa] = shape.fill;
    let [_, _, _, sa] = shape.stroke;
    let has_fill = fa > 0;
    let has_stroke = sa > 0 && shape.stroke_width > 0.0;
    let fill_attr = if has_fill {
        format!(
            "fill=\"{}\" fill-opacity=\"{}\"",
            hex(shape.fill),
            n(alpha(shape.fill))
        )
    } else {
        "fill=\"none\"".to_owned()
    };
    let stroke_attr = if has_stroke {
        format!(
            "stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"",
            hex(shape.stroke),
            n(alpha(shape.stroke)),
            n(f64::from(shape.stroke_width)),
        )
    } else {
        String::new()
    };
    match shape.kind {
        ShapeKind::Rect => {
            svg.push_str(&format!(
                "<rect width=\"{w}\" height=\"{h}\" rx=\"{rx}\" {fill} {stroke}/>\n",
                w = n(w),
                h = n(h),
                rx = n(f64::from(shape.corner_radius.max(0.0))),
                fill = fill_attr,
                stroke = stroke_attr,
            ));
        }
        ShapeKind::Ellipse => {
            svg.push_str(&format!(
                "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\" {fill} {stroke}/>\n",
                cx = n(w / 2.0),
                cy = n(h / 2.0),
                rx = n(w / 2.0),
                ry = n(h / 2.0),
                fill = fill_attr,
                stroke = stroke_attr,
            ));
        }
        ShapeKind::Line => {
            // La línea usa el borde si lo hay; si no, el relleno (scene.rs:314).
            let color = if has_stroke {
                format!(
                    "stroke=\"{}\" stroke-opacity=\"{}\"",
                    hex(shape.stroke),
                    n(alpha(shape.stroke))
                )
            } else {
                format!(
                    "stroke=\"{}\" stroke-opacity=\"{}\"",
                    hex(shape.fill),
                    n(alpha(shape.fill))
                )
            };
            svg.push_str(&format!(
                "<line x1=\"0\" y1=\"{y}\" x2=\"{w}\" y2=\"{y}\" {color} stroke-width=\"{sw}\"/>\n",
                y = n(h / 2.0),
                w = n(w),
                color = color,
                sw = n(f64::from(shape.stroke_width.max(0.0))),
            ));
        }
    }
}

fn hex([r, g, b, _]: [u8; 4]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn alpha([_, _, _, a]: [u8; 4]) -> f64 {
    f64::from(a) / 255.0
}

/// Escapa `&`, `<`, `>` y `"` para texto/atributos XML.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Formatea un número recortado a 4 decimales, sin ceros sobrantes. NaN/inf
/// se convierten en `0` (un SVG con un `NaN` en un atributo numérico no es
/// válido y algunos visores lo rechazan entero).
fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let rounded = (v * 10_000.0).round() / 10_000.0;
    let s = format!("{rounded:.4}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::{ImageContent, ShapeContent, TextAlign};

    fn stub_text_lines<'a>(text: &'a str) -> impl Fn(&TextContent, f64) -> Vec<TextLine> + 'a {
        move |content, _box_width| {
            vec![TextLine {
                text: text.to_owned(),
                x: 0.0,
                baseline: f64::from(content.size),
            }]
        }
    }

    fn doc_with_shape_text_and_image() -> (Document, ExportImages) {
        let mut doc = Document::new(200.0, 100.0);
        doc.page_mut().unwrap().background = Some([255, 255, 255, 255]);
        doc.add_layer(
            "shape",
            Transform::new(10.0, 10.0, 40.0, 20.0),
            LayerContent::Shape(ShapeContent {
                kind: ShapeKind::Rect,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
                corner_radius: 2.0,
            }),
        )
        .unwrap();
        let text_id = doc
            .add_layer(
                "text",
                Transform::new(10.0, 40.0, 100.0, 30.0),
                LayerContent::Text(TextContent {
                    text: "Hola".to_owned(),
                    family: String::new(),
                    size: 16.0,
                    weight: 400,
                    italic: false,
                    letter_spacing: 0.0,
                    line_height: 1.2,
                    align: TextAlign::Left,
                    color: [10, 10, 10, 255],
                }),
            )
            .unwrap();
        let img_id = doc
            .add_layer(
                "img",
                Transform::new(60.0, 10.0, 20.0, 20.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 2,
                    natural_height: 2,
                    crop: None,
                }),
            )
            .unwrap();
        let mut images = ExportImages::new();
        // Un PNG 2x2 real, mínimo, para probar el <image>.
        let rgba: Vec<u8> = (0..2 * 2 * 4).map(|i| (i * 17 % 256) as u8).collect();
        let png_base64 = crate::encode_layer_png(&rgba, 2, 2).unwrap();
        images.insert(img_id.raw(), png_base64);
        let _ = text_id;
        (doc, images)
    }

    #[test]
    fn svg_roundtrips_through_usvg() {
        let (doc, images) = doc_with_shape_text_and_image();
        let breaker = stub_text_lines("Hola");
        let svg = document_to_svg(&doc, &images, 1.0, &breaker).unwrap();

        let mut options = resvg::usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        let tree = resvg::usvg::Tree::from_str(&svg, &options)
            .expect("el SVG generado debe ser válido para usvg");
        assert_eq!(tree.size().width(), 200.0);
        assert_eq!(tree.size().height(), 100.0);
    }

    #[test]
    fn svg_escapes_text_and_names() {
        let mut doc = Document::new(10.0, 10.0);
        doc.add_layer(
            "capa",
            Transform::new(0.0, 0.0, 10.0, 10.0),
            LayerContent::Text(TextContent {
                text: "<script>&\"".to_owned(),
                ..TextContent::default()
            }),
        )
        .unwrap();
        let breaker = stub_text_lines("<script>&\"");
        let svg = document_to_svg(&doc, &ExportImages::new(), 1.0, &breaker).unwrap();
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;&amp;&quot;"));
    }

    #[test]
    fn svg_image_layer_embeds_a_base64_png() {
        let (doc, images) = doc_with_shape_text_and_image();
        let breaker = stub_text_lines("Hola");
        let svg = document_to_svg(&doc, &images, 1.0, &breaker).unwrap();
        assert!(svg.contains("data:image/png;base64,"));
    }

    #[test]
    fn svg_scale_multiplies_the_declared_size_not_the_viewbox() {
        let (doc, images) = doc_with_shape_text_and_image();
        let breaker = stub_text_lines("Hola");
        let svg = document_to_svg(&doc, &images, 2.0, &breaker).unwrap();
        assert!(svg.contains("width=\"400\" height=\"200\" viewBox=\"0 0 200 100\""));
    }

    #[test]
    fn pdf_starts_with_the_pdf_header() {
        let (doc, images) = doc_with_shape_text_and_image();
        let breaker = stub_text_lines("Hola");
        let svg = document_to_svg(&doc, &images, 1.0, &breaker).unwrap();
        let pdf = svg_to_pdf(&svg).expect("conversion a PDF");
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn export_format_from_path_matches_the_extension() {
        assert_eq!(
            ExportFormat::from_path(Path::new("a.png")),
            Some(ExportFormat::Png)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("a.JPG")),
            Some(ExportFormat::Jpeg)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("a.svg")),
            Some(ExportFormat::Svg)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("a.pdf")),
            Some(ExportFormat::Pdf)
        );
        assert_eq!(ExportFormat::from_path(Path::new("a.txt")), None);
    }
}
