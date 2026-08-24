//! Modales del editor (sobrescritura destructiva, origen de solo lectura,
//! diálogo de exportación) y las ventanas flotantes de Ajustes/About.
//!
//! Los tres primeros son funciones libres, no métodos de `App`: se llaman
//! desde dentro de la rama `View::Editor` de `ui_views::editor_view_ui`,
//! donde `state` sigue prestado de `self.view` — igual que `editor::canvas_ui`,
//! reciben cada campo de `App` que necesitan por separado en vez de `&mut
//! self` entero. Ajustes/About sí son métodos: se llaman después de que el
//! `match` de la vista activa termina, así que `&mut self` está libre otra
//! vez.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_shell::ShellIntegration as _;
use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::loader::{self, AppMsg};
use crate::{editor, export, settings};

use super::persistence::{is_jpeg_path, start_save, SaveContext};
use super::{App, Nav};

impl App {
    /// Ventana de ajustes (accesible desde la bienvenida y el editor).
    pub(super) fn settings_window_ui(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let before = self.settings.clone();
        let action = settings::settings_window(
            ctx,
            &mut self.settings,
            &mut self.show_settings,
            &self.shell_status,
        );
        if self.settings != before {
            self.settings.save_in_background();
        }
        if let Some(action) = action {
            self.shell_status = "Working…".to_owned();
            let tx = self.tx.clone();
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let shell = canvas_shell::platform();
                let result = match action {
                    settings::SettingsAction::RegisterShell => std::env::current_exe()
                        .map_err(|e| e.to_string())
                        .and_then(|exe| {
                            shell
                                .register_file_associations(&exe)
                                .map(|()| {
                                    "Explorer integration registered. Right-click an \
                                     image → Open with → Canvas Desktop."
                                        .to_owned()
                                })
                                .map_err(|e| e.to_string())
                        }),
                    settings::SettingsAction::UnregisterShell => shell
                        .unregister_file_associations()
                        .map(|()| "Explorer integration removed.".to_owned())
                        .map_err(|e| e.to_string()),
                };
                let _ = tx.send(AppMsg::ShellIntegrationDone(result));
                ctx2.request_repaint();
            });
        }
    }

    /// Ventana «About» (menú Help).
    pub(super) fn about_window_ui(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        egui::Window::new("About Canvas Desktop")
            .open(&mut self.show_about)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Canvas Desktop {}", env!("CARGO_PKG_VERSION")));
                ui.weak("A native canvas editor that saves straight to your image files.");
            });
    }
}

/// Modal de aviso de sobrescritura destructiva, mostrado antes del primer
/// guardado en el sitio de un archivo raster que la app no creó ella misma.
#[allow(clippy::too_many_arguments)]
pub(super) fn overwrite_modal_ui(
    state: &mut editor::EditorState,
    renderer: &mut canvas_render::CanvasRenderer,
    rs: &RenderState,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
    settings: &mut settings::AppSettings,
    overwrite_prompt: &mut Option<PathBuf>,
    overwrite_confirmed: &mut bool,
    overwrite_dont_ask: &mut bool,
    close_after_save: &mut bool,
    after_save: &mut Option<Nav>,
    ignore_fs_events_until: &mut Option<std::time::Instant>,
) {
    let Some(path) = overwrite_prompt.clone() else {
        return;
    };
    enum Choice {
        None,
        Overwrite,
        SaveAs,
        Cancel,
    }
    let mut choice = Choice::None;
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let jpeg_quality = settings.jpeg_quality;
    let modal = egui::Modal::new(egui::Id::new("overwrite_warning")).show(ctx, |ui| {
        ui.set_max_width(400.0);
        ui.heading("Overwrite the original file?");
        ui.add_space(6.0);
        ui.label(format!(
            "Saving will permanently replace \"{file_name}\" on disk \
             with the edited result. This cannot be undone."
        ));
        if is_jpeg_path(&path) {
            ui.label(format!(
                "The JPEG will be re-encoded at quality {jpeg_quality}."
            ));
        }
        ui.add_space(8.0);
        ui.checkbox(overwrite_dont_ask, "Don't ask again");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Overwrite").clicked() {
                choice = Choice::Overwrite;
            }
            if ui.button("Save as… instead").clicked() {
                choice = Choice::SaveAs;
            }
            if ui.button("Cancel").clicked() {
                choice = Choice::Cancel;
            }
        });
    });
    // Clic fuera o Esc equivalen a cancelar.
    if modal.should_close() && matches!(choice, Choice::None) {
        choice = Choice::Cancel;
    }
    match choice {
        Choice::None => {}
        Choice::Overwrite => {
            *overwrite_prompt = None;
            *overwrite_confirmed = true;
            if *overwrite_dont_ask && !settings.skip_overwrite_warning {
                settings.skip_overwrite_warning = true;
                settings.save_in_background();
            }
            let mut sctx = SaveContext {
                renderer,
                rs,
                tx,
                ctx,
                ignore_fs_events_until,
            };
            start_save(state, &mut sctx, path, false, settings.jpeg_quality);
        }
        Choice::SaveAs => {
            *overwrite_prompt = None;
            if *overwrite_dont_ask && !settings.skip_overwrite_warning {
                settings.skip_overwrite_warning = true;
                settings.save_in_background();
            }
            loader::spawn_pick_save_path(Some(state.file_name()), tx.clone(), ctx.clone());
        }
        Choice::Cancel => {
            *overwrite_prompt = None;
            *close_after_save = false;
            *after_save = None;
        }
    }
}

/// Modal para SVG/GIF: no se pueden sobrescribir, se explica por qué y se
/// ofrece «Save as…» en su lugar.
pub(super) fn readonly_modal_ui(
    state: &mut editor::EditorState,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
    readonly_prompt: &mut Option<PathBuf>,
    close_after_save: &mut bool,
    after_save: &mut Option<Nav>,
) {
    let Some(path) = readonly_prompt.clone() else {
        return;
    };
    let mut save_as_instead = false;
    let mut cancel = false;
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let is_svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let modal = egui::Modal::new(egui::Id::new("readonly_source")).show(ctx, |ui| {
        ui.set_max_width(400.0);
        ui.heading("This file can't be overwritten");
        ui.add_space(6.0);
        if is_svg {
            ui.label(format!(
                "\"{file_name}\" is a vector SVG. Canvas Desktop edits \
                 raster pixels and can't rewrite vector artwork, so the \
                 original stays untouched."
            ));
        } else {
            ui.label(format!(
                "\"{file_name}\" is a GIF, which may be animated. \
                 Overwriting it would flatten the animation to a single \
                 frame, so the original stays untouched."
            ));
        }
        ui.label("Use \"Save as…\" to save the result as a new file.");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Save as…").clicked() {
                save_as_instead = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if modal.should_close() && !save_as_instead {
        cancel = true;
    }
    if save_as_instead {
        *readonly_prompt = None;
        loader::spawn_pick_save_path(Some(state.file_name()), tx.clone(), ctx.clone());
    } else if cancel {
        *readonly_prompt = None;
        *close_after_save = false;
        *after_save = None;
    }
}

/// Diálogo de exportación (elegir formato/escala) y arranque del hilo de
/// export en cuanto el usuario ya eligió también la ruta de destino.
#[allow(clippy::too_many_arguments)]
pub(super) fn export_flow_ui(
    state: &mut editor::EditorState,
    renderer: &mut canvas_render::CanvasRenderer,
    rs: &RenderState,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
    export_dialog: &mut Option<export::ExportDialog>,
    pending_export_settings: &mut Option<export::ExportSettings>,
    pending_export: &mut Option<(PathBuf, export::ExportSettings)>,
) {
    if let Some(dialog) = export_dialog {
        let page_size = state
            .doc
            .page()
            .map(|p| (p.width, p.height))
            .unwrap_or((0.0, 0.0));
        match export::export_modal(dialog, ctx, page_size) {
            export::ExportChoice::None => {}
            export::ExportChoice::Cancel => {
                *export_dialog = None;
            }
            export::ExportChoice::Pick(settings) => {
                *export_dialog = None;
                let stem = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".to_owned());
                let suggested = format!("{stem}.{}", settings.format.extension());
                loader::spawn_pick_export_path(suggested, settings.format, tx.clone(), ctx.clone());
                *pending_export_settings = Some(settings);
            }
        }
    }
    if let Some((path, settings)) = pending_export.take() {
        super::persistence::start_export(state, renderer, rs, tx, ctx, path, settings);
    }
}
