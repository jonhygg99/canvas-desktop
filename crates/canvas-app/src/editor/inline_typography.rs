//! Fuente del renderer y estilo de maquetación del campo de texto editable.
use canvas_core::{TextAlign, TextContent};
use eframe::egui;

pub(super) fn font(ctx: &egui::Context, text: &TextContent) -> Option<egui::FontFamily> {
    let name = format!(
        "canvas-font:{}:{}:{}",
        text.family, text.weight, text.italic
    );
    let id = egui::Id::new(&name);
    let family = egui::FontFamily::Name(name.clone().into());
    if ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return Some(family);
    }
    let Some(resolved) = canvas_render::text_font(text) else {
        return Some(egui::FontFamily::Proportional);
    };
    let mut data = egui::FontData::from_owned(resolved.data.data().to_vec());
    data.index = resolved.index;
    ctx.add_font(egui::epaint::text::FontInsert::new(
        &name,
        data,
        vec![egui::epaint::text::InsertFontFamily {
            family,
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    ));
    ctx.data_mut(|d| d.insert_temp(id, true));
    ctx.request_repaint();
    // egui instala la fuente al comienzo del siguiente fotograma.
    None
}

pub(super) fn job(
    text: &str,
    style: &TextContent,
    family: egui::FontFamily,
    zoom: f32,
    width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(style.size * zoom, family),
            extra_letter_spacing: style.letter_spacing * zoom,
            line_height: Some(style.size * style.line_height.max(0.5) * zoom),
            color: egui::Color32::from_rgba_unmultiplied(
                style.color[0],
                style.color[1],
                style.color[2],
                style.color[3],
            ),
            ..Default::default()
        },
    );
    job.wrap.max_width = width;
    job.halign = match style.align {
        TextAlign::Left => egui::Align::LEFT,
        TextAlign::Center => egui::Align::Center,
        TextAlign::Right => egui::Align::RIGHT,
    };
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_layout_preserves_color_alignment_and_scaled_spacing() {
        let style = TextContent {
            size: 40.0,
            letter_spacing: 3.0,
            line_height: 1.5,
            align: TextAlign::Right,
            color: [20, 40, 60, 255],
            ..Default::default()
        };
        let job = job("sample", &style, egui::FontFamily::Proportional, 0.5, 100.0);
        let format = &job.sections[0].format;
        assert_eq!(format.font_id.size, 20.0);
        assert_eq!(format.extra_letter_spacing, 1.5);
        assert_eq!(format.line_height, Some(30.0));
        assert_eq!(format.color.to_array(), style.color);
        assert_eq!(job.halign, egui::Align::RIGHT);
    }
}
