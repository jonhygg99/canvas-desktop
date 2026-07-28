//! Construcción de la escena vello a partir del documento.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use canvas_core::{Document, LayerContent, LayerId, ShapeKind, TextAlign, TextContent};
use vello::kurbo::{Affine, Rect};
use vello::peniko::color::palette;
use vello::peniko::{Blob, Color, Fill, ImageData};
use vello::Scene;

/// Colocación del rect de una capa: posición + rotación sobre el centro +
/// volteo sobre el centro.
fn place_transform(t: &canvas_core::Transform) -> Affine {
    let center = vello::kurbo::Point::new(t.width / 2.0, t.height / 2.0);
    let flip = Affine::translate((center.x, center.y))
        * Affine::scale_non_uniform(
            if t.flip_h { -1.0 } else { 1.0 },
            if t.flip_v { -1.0 } else { 1.0 },
        )
        * Affine::translate((-center.x, -center.y));
    Affine::translate((t.x, t.y)) * Affine::rotate_about(t.rotation.to_radians(), center) * flip
}

/// Contextos de parley reutilizados entre frames (crear un `FontContext`
/// enumera las fuentes del sistema: demasiado caro por frame).
struct TextCtx {
    fonts: parley::FontContext,
    layouts: parley::LayoutContext<[u8; 4]>,
}

fn text_ctx() -> &'static Mutex<TextCtx> {
    static CTX: OnceLock<Mutex<TextCtx>> = OnceLock::new();
    CTX.get_or_init(|| {
        Mutex::new(TextCtx {
            fonts: parley::FontContext::new(),
            layouts: parley::LayoutContext::new(),
        })
    })
}

/// Arma el layout de parley de un texto (line breaking + alineación): el
/// MISMO que usan `draw_text` (para pintar) y `text_lines` (para exportar a
/// SVG), así el SVG nunca puede desincronizarse de lo que se ve en el lienzo.
fn build_layout(
    fonts: &mut parley::FontContext,
    layouts: &mut parley::LayoutContext<[u8; 4]>,
    content: &TextContent,
    box_width: f64,
) -> parley::Layout<[u8; 4]> {
    let mut builder = layouts.ranged_builder(fonts, &content.text, 1.0, true);
    builder.push_default(parley::StyleProperty::FontSize(content.size.max(1.0)));
    if !content.family.is_empty() {
        builder.push_default(parley::StyleProperty::FontFamily(
            parley::FontFamily::named(content.family.as_str()),
        ));
    }
    builder.push_default(parley::StyleProperty::FontWeight(parley::FontWeight::new(
        f32::from(content.weight),
    )));
    if content.italic {
        builder.push_default(parley::StyleProperty::FontStyle(parley::FontStyle::Italic));
    }
    builder.push_default(parley::StyleProperty::LetterSpacing(content.letter_spacing));
    builder.push_default(parley::StyleProperty::LineHeight(
        parley::LineHeight::FontSizeRelative(content.line_height.max(0.5)),
    ));
    let mut layout = builder.build(&content.text);
    layout.break_all_lines(Some(box_width.max(1.0) as f32));
    let align = match content.align {
        TextAlign::Left => parley::Alignment::Start,
        TextAlign::Center => parley::Alignment::Center,
        TextAlign::Right => parley::Alignment::End,
    };
    layout.align(align, parley::AlignmentOptions::default());
    layout
}

/// Pinta una capa de texto: layout con parley, glifos con vello.
fn draw_text(scene: &mut Scene, transform: Affine, content: &TextContent, box_width: f64) {
    let Ok(mut ctx) = text_ctx().lock() else {
        return;
    };
    let TextCtx { fonts, layouts } = &mut *ctx;
    let layout = build_layout(fonts, layouts, content, box_width);

    let [r, g, b, a] = content.color;
    let brush = Color::from_rgba8(r, g, b, a);
    for line in layout.lines() {
        for item in line.items() {
            let parley::PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let font = run.run().font().clone();
            let font_size = run.run().font_size();
            let coords = run.run().normalized_coords().to_vec();
            let glyphs: Vec<vello::Glyph> = run
                .positioned_glyphs()
                .map(|glyph| vello::Glyph {
                    id: glyph.id,
                    x: glyph.x,
                    y: glyph.y,
                })
                .collect();
            scene
                .draw_glyphs(&font)
                .font_size(font_size)
                .normalized_coords(&coords)
                .brush(brush)
                .transform(transform)
                .draw(Fill::NonZero, glyphs.into_iter());
        }
    }
}

/// Métricas de las líneas de un texto tal y como las rompe parley, en
/// coordenadas locales de la caja (el MISMO layout que pinta `draw_text`).
/// Lo usa la exportación a SVG para emitir un `<tspan x y>` por línea, sin
/// que el renderer SVG tenga que tomar ninguna decisión de salto de línea ni
/// de alineación por su cuenta: `x` ya lleva el desplazamiento de
/// alineación (`LineMetrics::offset`) y `y` es la línea base.
pub fn text_lines(content: &TextContent, box_width: f64) -> Vec<canvas_core::TextLine> {
    let Ok(mut ctx) = text_ctx().lock() else {
        return Vec::new();
    };
    let TextCtx { fonts, layouts } = &mut *ctx;
    let layout = build_layout(fonts, layouts, content, box_width);
    layout
        .lines()
        .map(|line| {
            let metrics = line.metrics();
            let text = content
                .text
                .get(line.text_range())
                .unwrap_or("")
                .trim_end_matches(['\n', '\r'])
                .to_owned();
            canvas_core::TextLine {
                text,
                x: f64::from(metrics.offset),
                baseline: f64::from(metrics.baseline),
            }
        })
        .collect()
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
fn checker_image() -> &'static ImageData {
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

/// Construye la escena del documento con la transformación de vista dada
/// (página → píxeles físicos del lienzo). `blurred` sustituye la imagen de
/// las capas con desenfoque activo (textura GPU ya procesada).
///
/// Con `decorated` se pintan los adornos de edición (tablero de transparencia
/// y borde de página); el horneado para guardar/exportar va sin ellos.
pub fn build_scene(
    doc: &Document,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    decorated: bool,
) -> Scene {
    let mut scene = Scene::new();
    let Ok(page) = doc.page() else {
        return scene;
    };
    let page_rect = Rect::new(0.0, 0.0, page.width, page.height);

    // Fondo de la página: color sólido o tablero de transparencia.
    match page.background {
        Some([r, g, b, a]) => {
            scene.fill(
                Fill::NonZero,
                view,
                vello::peniko::Color::from_rgba8(r, g, b, a),
                None,
                &page_rect,
            );
        }
        None if !decorated => {}
        None => {
            // El tablero se dibuja en coordenadas de pantalla (tamaño de
            // celda constante al hacer zoom): rellena el rect de la página
            // proyectado, con la imagen 2x2 repetida y escalada a 8px/celda.
            let brush = vello::peniko::ImageBrush {
                image: checker_image().clone(),
                sampler: vello::peniko::ImageSampler {
                    x_extend: vello::peniko::Extend::Repeat,
                    y_extend: vello::peniko::Extend::Repeat,
                    quality: vello::peniko::ImageQuality::Low,
                    alpha: 1.0,
                },
            };
            scene.fill(
                Fill::NonZero,
                view,
                &brush,
                Some(Affine::scale(8.0)),
                &page_rect,
            );
        }
    }

    // Capas, de abajo arriba, recortadas al rect de la página (lo que
    // sobresale del lienzo no se ve ni se hornea). Se recorre con un índice
    // (no un `for … in`) porque los grupos necesitan abrir/cerrar su propia
    // capa de opacidad alrededor de todo su subárbol contiguo (invariante de
    // preorden de `Page`): `open` lleva, por cada grupo abierto, el índice de
    // su último descendiente y si de verdad se empujó una capa de opacidad
    // (alpha < 1.0 — el camino rápido con alpha == 1.0 no cuesta nada extra).
    // Las hojas hacen lo mismo, más ceñido, alrededor de sombra+contenido.
    // Los `push_layer` anidados se multiplican solos: así `Layer::opacity` se
    // honra de forma uniforme y estructural en las cuatro clases de capa.
    scene.push_layer(
        Fill::NonZero,
        vello::peniko::Mix::Normal,
        1.0,
        view,
        &page_rect,
    );
    let mut open: Vec<(usize, bool)> = Vec::new();
    let mut i = 0usize;
    while i < page.layers.len() {
        while open.last().is_some_and(|&(end, _)| i > end) {
            let (_, pushed) = open.pop().expect("comprobado en la condición del while");
            if pushed {
                scene.pop_layer();
            }
        }
        let layer = &page.layers[i];
        let len = page.subtree_len(i);
        if !layer.visible {
            i += 1 + len; // un grupo oculto se salta entero, con sus hijos
            continue;
        }

        let alpha = layer.opacity.clamp(0.0, 1.0);
        let fade = alpha < 1.0;
        if fade {
            scene.push_layer(
                Fill::NonZero,
                vello::peniko::Mix::Normal,
                alpha,
                view,
                &page_rect,
            );
        }

        if matches!(layer.content, LayerContent::Group(_)) {
            // El grupo no pinta nada por sí mismo: solo deja abierta su capa
            // de opacidad (si la hay) hasta que se recorra todo su subárbol.
            open.push((i + len, fade));
            i += 1;
            continue;
        }

        // Sombra proyectada (rectangular, difusa) por debajo de la capa.
        if let Some(shadow) = layer.effects.shadow {
            let t = layer.transform;
            let rect = Rect::new(
                t.x + shadow.offset_x,
                t.y + shadow.offset_y,
                t.x + t.width + shadow.offset_x,
                t.y + t.height + shadow.offset_y,
            );
            scene.draw_blurred_rounded_rect(
                view,
                rect,
                vello::peniko::Color::BLACK.with_alpha(shadow.opacity.clamp(0.0, 1.0)),
                0.0,
                f64::from(shadow.blur.max(0.0)),
            );
        }
        match &layer.content {
            LayerContent::Image(content) => {
                let Some(image) = blurred.get(&layer.id).or_else(|| images.get(&layer.id)) else {
                    continue;
                };
                if image.width == 0 || image.height == 0 {
                    continue;
                }
                let t = layer.transform;
                let place = place_transform(&t);

                // Recorte no destructivo: la fracción `crop` del mapa de bits
                // llena el rect; el resto se recorta con una capa de clip.
                let crop = content
                    .crop
                    .map(canvas_core::CropRect::clamped)
                    .unwrap_or_else(canvas_core::CropRect::full);
                let (iw, ih) = (f64::from(image.width), f64::from(image.height));
                let sx = t.width / (crop.width * iw);
                let sy = t.height / (crop.height * ih);
                let image_local = Affine::scale_non_uniform(sx, sy)
                    * Affine::translate((-crop.x * iw, -crop.y * ih));

                let cropped = content.crop.is_some();
                if cropped {
                    scene.push_layer(
                        Fill::NonZero,
                        vello::peniko::Mix::Normal,
                        1.0,
                        view * place,
                        &Rect::new(0.0, 0.0, t.width, t.height),
                    );
                }
                scene.draw_image(image, view * place * image_local);
                if cropped {
                    scene.pop_layer();
                }
            }
            // El SVG pinta sus píxeles rasterizados del ImageMap (la fuente
            // vectorial viaja en el documento para reexportar sin pérdida).
            LayerContent::Svg(_) => {
                let Some(image) = blurred.get(&layer.id).or_else(|| images.get(&layer.id)) else {
                    continue;
                };
                if image.width == 0 || image.height == 0 {
                    continue;
                }
                let t = layer.transform;
                let place = place_transform(&t);
                let image_local = Affine::scale_non_uniform(
                    t.width / f64::from(image.width),
                    t.height / f64::from(image.height),
                );
                scene.draw_image(image, view * place * image_local);
            }
            LayerContent::Text(text) => {
                let t = layer.transform;
                draw_text(&mut scene, view * place_transform(&t), text, t.width);
            }
            LayerContent::Shape(shape) => {
                let t = layer.transform;
                let place = view * place_transform(&t);
                let [fr, fg, fb, fa] = shape.fill;
                let [sr, sg, sb, sa] = shape.stroke;
                let fill_color = Color::from_rgba8(fr, fg, fb, fa);
                let stroke_color = Color::from_rgba8(sr, sg, sb, sa);
                let stroke = vello::kurbo::Stroke::new(f64::from(shape.stroke_width.max(0.0)));
                match shape.kind {
                    ShapeKind::Rect => {
                        let rounded = vello::kurbo::RoundedRect::from_rect(
                            Rect::new(0.0, 0.0, t.width, t.height),
                            f64::from(shape.corner_radius.max(0.0)),
                        );
                        if fa > 0 {
                            scene.fill(Fill::NonZero, place, fill_color, None, &rounded);
                        }
                        if sa > 0 && shape.stroke_width > 0.0 {
                            scene.stroke(&stroke, place, stroke_color, None, &rounded);
                        }
                    }
                    ShapeKind::Ellipse => {
                        let ellipse = vello::kurbo::Ellipse::new(
                            (t.width / 2.0, t.height / 2.0),
                            (t.width / 2.0, t.height / 2.0),
                            0.0,
                        );
                        if fa > 0 {
                            scene.fill(Fill::NonZero, place, fill_color, None, &ellipse);
                        }
                        if sa > 0 && shape.stroke_width > 0.0 {
                            scene.stroke(&stroke, place, stroke_color, None, &ellipse);
                        }
                    }
                    ShapeKind::Line => {
                        let line = vello::kurbo::Line::new(
                            (0.0, t.height / 2.0),
                            (t.width, t.height / 2.0),
                        );
                        let color = if sa > 0 { stroke_color } else { fill_color };
                        scene.stroke(&stroke, place, color, None, &line);
                    }
                }
            }
            // Los grupos se gestionan más arriba (con `continue`, antes de
            // sombra+contenido): esta rama nunca se alcanza para ellos.
            LayerContent::Group(_) => unreachable!("los grupos no llegan a pintarse aquí"),
        }

        if fade {
            scene.pop_layer();
        }
        i += 1;
    }
    // Cierra cualquier grupo que quedara abierto al final de la página (no
    // debería quedar ninguno si la invariante de preorden se cumple, pero un
    // documento restaurado de un sidecar corrupto no puede colgar el hilo de
    // render por un `push_layer`/`pop_layer` desbalanceado).
    while let Some((_, pushed)) = open.pop() {
        if pushed {
            scene.pop_layer();
        }
    }
    debug_assert!(open.is_empty());

    scene.pop_layer();

    // Borde sutil de la página por encima de todo (solo en pantalla).
    if decorated {
        scene.stroke(
            &vello::kurbo::Stroke::new(1.0),
            view,
            palette::css::BLACK.with_alpha(0.25),
            None,
            &page_rect,
        );
    }

    scene
}
