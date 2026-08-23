//! Emision de una capa de texto como `<text>` con un `<tspan>` por linea. Las
//! metricas las da el llamador (`canvas_render::text_lines`): el visor de SVG
//! no debe decidir donde parte cada linea.

use canvas_core::TextContent;

use super::super::TextLineBreaker;

use super::util::{alpha, esc, hex, n};

/// `<text>` con un `<tspan x y>` por línea (`text_lines`, las MISMAS
/// métricas de parley que ve el lienzo): así el SVG no toma ninguna
/// decisión de salto de línea ni de alineación por su cuenta.
pub(super) fn text_element(
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
