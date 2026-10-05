//! Sistema visual nativo: métricas coherentes en ambos temas y preferencias.
use crate::settings::{AppSettings, Density};
use eframe::egui;

pub fn apply(ctx: &egui::Context, settings: &AppSettings) {
    let scale = normalized_scale(settings.ui_scale);
    let prefs = (settings.density, scale, settings.reduced_motion);
    let id = egui::Id::new("ui-appearance");
    if ctx.data(|d| d.get_temp::<(Density, f32, bool)>(id)) == Some(prefs) {
        return;
    }
    ctx.set_zoom_factor(scale);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| configure(style, theme, settings));
    }
    ctx.data_mut(|d| d.insert_temp(id, prefs));
}

pub(crate) fn normalized_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.75, 2.0)
    } else {
        1.0
    }
}

fn configure(style: &mut egui::Style, theme: egui::Theme, settings: &AppSettings) {
    let compact = settings.density == Density::Compact;
    let height = if compact { 24.0 } else { 30.0 };
    style.spacing.interact_size = egui::vec2(height, height);
    style.spacing.item_spacing = if compact {
        egui::vec2(6.0, 4.0)
    } else {
        egui::vec2(8.0, 6.0)
    };
    style.spacing.button_padding = if compact {
        egui::vec2(6.0, 3.0)
    } else {
        egui::vec2(8.0, 5.0)
    };
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(if compact { 13.0 } else { 14.0 }),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::proportional(if compact { 13.0 } else { 14.0 }),
    );
    style.animation_time = if settings.reduced_motion { 0.0 } else { 0.15 };
    let dark = theme == egui::Theme::Dark;
    let visuals = &mut style.visuals;
    visuals.panel_fill = if dark {
        egui::Color32::from_rgb(27, 29, 33)
    } else {
        egui::Color32::from_rgb(245, 246, 248)
    };
    visuals.window_fill = visuals.panel_fill;
    visuals.extreme_bg_color = if dark {
        egui::Color32::from_rgb(18, 20, 23)
    } else {
        egui::Color32::from_rgb(255, 255, 255)
    };
    visuals.override_text_color = Some(if dark {
        egui::Color32::from_rgb(235, 238, 242)
    } else {
        egui::Color32::from_rgb(29, 34, 43)
    });
    visuals.selection.bg_fill = if dark {
        egui::Color32::from_rgb(36, 80, 135)
    } else {
        egui::Color32::from_rgb(204, 224, 250)
    };
    visuals.selection.stroke = egui::Stroke::new(
        1.5,
        if dark {
            egui::Color32::from_rgb(121, 179, 255)
        } else {
            egui::Color32::from_rgb(28, 87, 165)
        },
    );
}

#[cfg(test)]
mod tests;
