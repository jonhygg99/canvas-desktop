//! Tests de exportacion: sobre todo que el SVG generado es correcto y que el
//! texto sale como `<text>`/`<tspan>` de verdad.

use super::*;
use canvas_core::{
    Document, ImageContent, LayerContent, ShapeContent, ShapeKind, TextAlign, Transform,
};

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
