use super::AppSettings;
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

pub(super) fn controls(ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.label(crate::i18n::tr("Language"));
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut settings.language,
            crate::i18n::Language::English,
            "English",
        );
        ui.selectable_value(
            &mut settings.language,
            crate::i18n::Language::Spanish,
            "Español",
        );
    });
    ui.label(crate::i18n::tr("Interface density"));
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut settings.density,
            Density::Comfortable,
            crate::i18n::tr("Comfortable"),
        );
        ui.selectable_value(
            &mut settings.density,
            Density::Compact,
            crate::i18n::tr("Compact"),
        );
    });
    settings.ui_scale = crate::ui_style::normalized_scale(settings.ui_scale);
    ui.add(
        egui::Slider::new(&mut settings.ui_scale, 0.75..=2.0)
            .text(crate::i18n::tr("Interface scale"))
            .step_by(0.05),
    );
    if ui
        .button(crate::i18n::tr("Reset interface scale"))
        .clicked()
    {
        settings.ui_scale = 1.0;
    }
    ui.checkbox(
        &mut settings.reduced_motion,
        crate::i18n::tr("Reduce motion"),
    );
    ui.separator();
}
