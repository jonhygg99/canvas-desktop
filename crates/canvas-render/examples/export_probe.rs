//! Verificación headless de la Fase 11 (exportación): escala 2x exacta en
//! píxeles, opacidad de grupo horneada de verdad, ruptura de línea de texto
//! (`text_lines`) y que el SVG que genera `canvas_io::document_to_svg` sea
//! válido de verdad (se reparsea con usvg, sin necesitar GPU para eso).
//!
//! Uso: cargo run -p canvas-render --example export_probe

use anyhow::{anyhow, Result};
use canvas_core::{
    Document, ImageContent, Layer, LayerContent, ShapeContent, ShapeKind, TextAlign, TextContent,
    Transform,
};
use canvas_render::{text_lines, CanvasRenderer, ImageMap};
use vello::util::RenderContext;

/// Un único rectángulo rojo opaco a tamaño completo de página.
fn doc_with_plain_shape(w: u32, h: u32) -> Result<Document> {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    doc.add_layer(
        "shape",
        Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
        LayerContent::Shape(ShapeContent {
            kind: ShapeKind::Rect,
            fill: [255, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
            corner_radius: 0.0,
        }),
    )?;
    Ok(doc)
}

/// El mismo rectángulo, pero dentro de un grupo a opacidad 0.5.
fn doc_with_grouped_shape(w: u32, h: u32) -> Result<Document> {
    let mut doc = doc_with_plain_shape(w, h)?;
    let shape_id = doc.page()?.layers[0].id;
    let group_id = doc.allocate_layer_id();
    let page = doc.page_mut()?;
    page.insert_child(Layer::group(group_id, "Group"), None, 0);
    page.move_subtree(shape_id, Some(group_id), 0)?;
    if let Some(g) = page.layer_mut(group_id) {
        g.opacity = 0.5;
    }
    Ok(doc)
}

fn main() -> Result<()> {
    let (w, h) = (64u32, 64u32);

    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);
    let mut renderer = CanvasRenderer::new(device)?;
    let images = ImageMap::new();

    // 1) Escala: 2x debe duplicar EXACTAMENTE cada dimensión y cuadruplicar
    // los bytes (criterio de PLAN.md: "Export PNG 2x produce exactamente el
    // doble de píxeles").
    let control_doc = doc_with_plain_shape(w, h)?;
    let (rgba1, w1, h1) = renderer.bake_page(
        device,
        queue,
        canvas_render::FxScope::default(),
        &control_doc,
        &images,
        1.0,
    )?;
    let (rgba2, w2, h2) = renderer.bake_page(
        device,
        queue,
        canvas_render::FxScope::default(),
        &control_doc,
        &images,
        2.0,
    )?;
    anyhow::ensure!(
        w2 == w1 * 2 && h2 == h1 * 2,
        "2x debería duplicar cada dimensión exactamente, dio {w1}x{h1} -> {w2}x{h2}"
    );
    anyhow::ensure!(
        rgba2.len() == rgba1.len() * 4,
        "2x debería cuadruplicar los bytes"
    );
    println!("scale OK: {w1}x{h1} -> {w2}x{h2} a 2x");

    // 2) Opacidad de grupo: un rectángulo opaco dentro de un grupo al 50 %
    // debe hornear con alpha≈128, frente a 255 del mismo rectángulo suelto.
    let center = (((h1 / 2) * w1 + w1 / 2) * 4) as usize;
    let control_alpha = rgba1[center + 3];
    anyhow::ensure!(
        control_alpha == 255,
        "el control (sin grupo) debería dar alpha 255, dio {control_alpha}"
    );
    let grouped_doc = doc_with_grouped_shape(w, h)?;
    let (grouped_rgba, gw, gh) = renderer.bake_page(
        device,
        queue,
        canvas_render::FxScope::default(),
        &grouped_doc,
        &images,
        1.0,
    )?;
    let gcenter = (((gh / 2) * gw + gw / 2) * 4) as usize;
    let grouped_alpha = grouped_rgba[gcenter + 3];
    anyhow::ensure!(
        (110..=145).contains(&grouped_alpha),
        "un grupo al 50% debería dar alpha≈128, dio {grouped_alpha}"
    );
    println!("group opacity OK: control alpha={control_alpha}, grupo al 50% alpha={grouped_alpha}");

    // 3) text_lines: un texto largo en una caja estrecha debe romper en
    // varias líneas, con línea base estrictamente creciente (cada línea más
    // abajo que la anterior).
    let content = TextContent {
        text: "Canvas Desktop exporta texto envuelto en varias líneas".to_owned(),
        family: String::new(),
        size: 32.0,
        weight: 400,
        italic: false,
        letter_spacing: 0.0,
        line_height: 1.2,
        align: TextAlign::Left,
        color: [20, 20, 20, 255],
    };
    let lines = text_lines(&content, 200.0);
    anyhow::ensure!(
        lines.len() >= 2,
        "un texto largo en una caja estrecha debería romper en >=2 líneas, dio {}",
        lines.len()
    );
    let increasing = lines
        .windows(2)
        .all(|pair| pair[1].baseline > pair[0].baseline);
    anyhow::ensure!(increasing, "las líneas deben tener línea base creciente");
    println!(
        "text_lines OK: {} líneas, línea base creciente",
        lines.len()
    );

    // 4) SVG: lo que genera document_to_svg debe ser un SVG de verdad, no
    // solo texto bien formado a ojo — se reparsea con usvg (sin GPU).
    let mut svg_doc = Document::new(f64::from(w), f64::from(h));
    let img_id = svg_doc.add_layer(
        "img",
        Transform::new(0.0, 0.0, 20.0, 20.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 2,
            natural_height: 2,
            crop: None,
        }),
    )?;
    let img_rgba: Vec<u8> = (0..2 * 2 * 4).map(|i| (i * 23 % 256) as u8).collect();
    let png_base64 = canvas_io::encode_layer_png(&img_rgba, 2, 2).map_err(|e| anyhow!("{e}"))?;
    let mut export_images = canvas_io::ExportImages::new();
    export_images.insert(img_id.raw(), png_base64);

    let svg = canvas_io::document_to_svg(&svg_doc, &export_images, 1.0, &text_lines)
        .map_err(|e| anyhow!("{e}"))?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(&svg, &options)
        .map_err(|e| anyhow!("el SVG generado no es válido para usvg: {e}"))?;
    anyhow::ensure!(
        (tree.size().width() - w as f32).abs() < 0.01,
        "el tamaño del SVG reparseado no coincide con la página"
    );
    println!(
        "SVG roundtrip OK: reparseado con usvg, {}x{}",
        tree.size().width(),
        tree.size().height()
    );

    println!("EXPORT_PROBE=ok (escala 2x exacta, opacidad de grupo, text_lines, SVG válido)");
    Ok(())
}
