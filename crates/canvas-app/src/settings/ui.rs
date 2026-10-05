use super::{
    appearance, AppSettings, BulkCanvasSize, NewCanvasFormat, SettingsAction, ThemeChoice,
};
use eframe::egui;

/// Ventana flotante de ajustes. El llamador detecta cambios comparando el
/// estado antes/después y persiste si procede. `shell_status` es el resultado
/// del último registro/desregistro, para mostrarlo.
pub fn settings_window(
    ctx: &egui::Context,
    settings: &mut AppSettings,
    open: &mut bool,
    shell_status: &str,
) -> Option<SettingsAction> {
    let mut action = None;
    egui::Window::new(crate::i18n::tr("Settings"))
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(460.0, 560.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                appearance::controls(ui, settings);
                ui.label(crate::i18n::tr("Theme"));
                ui.horizontal(|ui| {
                    for choice in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
                        ui.selectable_value(&mut settings.theme, choice, choice.label());
                    }
                });
                ui.add_space(10.0);

                ui.label(crate::i18n::tr("New canvas format"));
                egui::ComboBox::from_id_salt("new_canvas_format")
                    .selected_text(settings.new_canvas_format.label())
                    .show_ui(ui, |ui| {
                        for choice in [
                            NewCanvasFormat::Png,
                            NewCanvasFormat::Jpeg,
                            NewCanvasFormat::WebP,
                            NewCanvasFormat::Canvas,
                        ] {
                            ui.selectable_value(
                                &mut settings.new_canvas_format,
                                choice,
                                choice.label(),
                            );
                        }
                    });
                ui.weak(crate::i18n::tr(
                    "What \"New design\" and the \"+\" canvas create: a real image file \
                 (with its layers kept editable in a sidecar) or a standalone .canvas design.",
                ));
                ui.add_space(10.0);

                ui.label(crate::i18n::tr("Web bulk canvas size"));
                egui::ComboBox::from_id_salt("serper_bulk_size")
                    .selected_text(settings.serper_bulk_size.label())
                    .show_ui(ui, |ui| {
                        for choice in BulkCanvasSize::ALL {
                            ui.selectable_value(
                                &mut settings.serper_bulk_size,
                                choice,
                                choice.label(),
                            );
                        }
                    });
                ui.weak(crate::i18n::tr(
                    "Page size of every canvas created by \"Add\" in Select web images: \
                 all canvases in the batch measure the same.",
                ));
                ui.add_space(10.0);

                ui.label(crate::i18n::tr("JPEG quality when saving"));
                ui.add(egui::Slider::new(&mut settings.jpeg_quality, 1..=100).show_value(true));
                ui.weak(crate::i18n::tr(
                    "Overwriting a JPEG re-encodes it; higher quality = larger file.",
                ));
                ui.add_space(10.0);

                let mut ask = !settings.skip_overwrite_warning;
                if ui
                    .checkbox(
                        &mut ask,
                        crate::i18n::tr("Ask before overwriting the original file"),
                    )
                    .on_hover_text(crate::i18n::tr(
                        "Shows a warning the first time you save over the original \
                     image in each session.",
                    ))
                    .changed()
                {
                    settings.skip_overwrite_warning = !ask;
                }
                ui.add_space(10.0);

                explorer_section(ui, shell_status, &mut action);
            });
        });
    action
}

/// Sección «File Explorer integration» de la ventana de ajustes: registro y
/// limpieza de las asociaciones «Open with» (en plataformas sin shell
/// integration el botón reporta el error por `shell_status`).
fn explorer_section(ui: &mut egui::Ui, shell_status: &str, action: &mut Option<SettingsAction>) {
    ui.add_space(12.0);
    ui.separator();
    ui.label(crate::i18n::tr("File Explorer integration"));
    ui.weak(crate::i18n::tr(
        "Adds Canvas Desktop to \"Open with\" for images and to the \
         right-click menu of folders.",
    ));
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::tr("Register")).clicked() {
            *action = Some(SettingsAction::RegisterShell);
        }
        if ui.button(crate::i18n::tr("Unregister")).clicked() {
            *action = Some(SettingsAction::UnregisterShell);
        }
    });
    if !shell_status.is_empty() {
        ui.weak(shell_status);
    }
}
