//! Maquetado y pintado de texto con parley. `parley` comparte `peniko` con
//! vello (ver CLAUDE.md): tras subir vello hay que revalidar el par con
//! `cargo tree -i peniko` y el ejemplo `text_probe`.

use std::sync::{Mutex, OnceLock};

use canvas_core::{TextAlign, TextContent};
use vello::kurbo::Affine;
use vello::peniko::{Color, Fill};
use vello::Scene;

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
pub(super) fn draw_text(
    scene: &mut Scene,
    transform: Affine,
    content: &TextContent,
    box_width: f64,
) {
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
