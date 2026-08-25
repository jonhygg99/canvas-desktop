//! Sección «Página»: resolución (campos + presets), fondo desenfocado, y la
//! ventanita flotante "Size" del menú contextual del lienzo.

use canvas_core::{LayerContent, SetPageSize};
use eframe::egui;

use super::EditorState;

/// Sección «Página»: resolución (campos + presets) y fondo desenfocado.
pub(crate) fn page_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    let Ok(page) = state.doc.page() else { return };
    let original = (page.width, page.height);
    let mut w = original.0;
    let mut h = original.1;
    let mut changed = false;
    let mut commit = false;

    ui.horizontal(|ui| {
        ui.label("W");
        let rw = ui.add(
            egui::DragValue::new(&mut w)
                .speed(2.0)
                .range(16.0..=16384.0)
                .max_decimals(0),
        );
        ui.label("H");
        let rh = ui.add(
            egui::DragValue::new(&mut h)
                .speed(2.0)
                .range(16.0..=16384.0)
                .max_decimals(0),
        );
        changed |= rw.changed() || rh.changed();
        commit |= rw.drag_stopped() || rw.lost_focus() || rh.drag_stopped() || rh.lost_focus();

        // Presets rápidos de resolución.
        let image_size = state.doc.page().ok().and_then(|p| {
            p.layers.iter().rev().find_map(|l| match &l.content {
                LayerContent::Image(img) if Some(l.id) != state.background_layer => {
                    Some((f64::from(img.natural_width), f64::from(img.natural_height)))
                }
                _ => None,
            })
        });
        if page_size_presets_ui(ui, &mut w, &mut h, image_size) {
            changed = true;
            commit = true;
        }
    });

    if changed
        && (w, h)
            != (state
                .doc
                .page()
                .map(|p| (p.width, p.height))
                .unwrap_or(original))
    {
        if state.page_edit.is_none() {
            state.page_edit = Some(original);
        }
        if let Ok(page) = state.doc.page_mut() {
            page.width = w.max(16.0);
            page.height = h.max(16.0);
        }
    }
    if commit {
        if let Some(before) = state.page_edit.take() {
            let after = state
                .doc
                .page()
                .map(|p| (p.width, p.height))
                .unwrap_or(before);
            if after != before {
                // El fondo desenfocado (si lo hay) se recoloca para seguir
                // cubriendo la página nueva, todo en UN paso de deshacer.
                let mut commands: Vec<Box<dyn canvas_core::Command>> =
                    vec![Box::new(SetPageSize { before, after })];
                if let Some(cmd) = state.resync_background_cover() {
                    commands.push(cmd);
                }
                state.push_undo_step(Box::new(canvas_core::Composite::new(
                    "Resize page",
                    commands,
                )));
            }
        }
    }

}

/// Selector compartido para la página y el cuadro contextual Size.
/// Devuelve `true` cuando el usuario eligió un tamaño.
fn page_size_presets_ui(
    ui: &mut egui::Ui,
    w: &mut f64,
    h: &mut f64,
    image_size: Option<(f64, f64)>,
) -> bool {
    let mut selected = false;
    egui::ComboBox::from_id_salt("page_presets")
        .selected_text("Presets")
        .width(72.0)
        .show_ui(ui, |ui| {
            let mut preset = |ui: &mut egui::Ui, label: &str, pw: f64, ph: f64| {
                if ui.selectable_label(false, label).clicked() {
                    *w = pw;
                    *h = ph;
                    selected = true;
                }
            };

            ui.strong("Social");
            preset(
                ui,
                "Vertical / Reels / Shorts (1080 × 1920)",
                1080.0,
                1920.0,
            );
            preset(ui, "Square / Facebook 1:1 (1080 × 1080)", 1080.0, 1080.0);
            preset(
                ui,
                "Instagram portrait / Facebook feed (1080 × 1350)",
                1080.0,
                1350.0,
            );
            preset(
                ui,
                "LinkedIn / Facebook landscape (1200 × 628)",
                1200.0,
                628.0,
            );
            preset(ui, "Pinterest vertical (1000 × 1500)", 1000.0, 1500.0);

            ui.separator();
            ui.strong("Branding");
            preset(ui, "YouTube channel logo (800 × 800)", 800.0, 800.0);
            preset(ui, "Facebook page profile (320 × 320)", 320.0, 320.0);
            preset(ui, "Facebook page cover (851 × 315)", 851.0, 315.0);

            ui.separator();
            ui.strong("Video");
            preset(ui, "Video Full HD (1920 × 1080)", 1920.0, 1080.0);
            preset(ui, "Video 4K (3840 × 2160)", 3840.0, 2160.0);
            preset(ui, "YouTube thumbnail (1280 × 720)", 1280.0, 720.0);
            preset(ui, "YouTube banner (2560 × 1440)", 2560.0, 1440.0);

            if let Some((iw, ih)) = image_size {
                ui.separator();
                ui.strong("Source");
                let label = format!("Image ({} × {})", iw as i64, ih as i64);
                preset(ui, &label, iw, ih);
            }
        });
    selected
}

/// Ventanita flotante "Size" pedida desde el menú contextual del lienzo
/// (`canvas_ui`, botón "Size"): un formulario aparte con W/H en vez de
/// arrastrar el `DragValue` del panel — mismo commit que `page_ui` (un solo
/// paso de deshacer, con el fondo desenfocado recolocado si lo hay). Apply
/// confirma, Cancel (o la X) cierra sin tocar el documento.
pub(in crate::editor) fn size_popup_ui(state: &mut EditorState, ctx: &egui::Context) {
    let Some((mut w, mut h)) = state.size_popup else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    egui::Window::new("Page size")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("W");
                ui.add(
                    egui::DragValue::new(&mut w)
                        .speed(2.0)
                        .range(16.0..=16384.0)
                        .max_decimals(0),
                );
                ui.label("H");
                ui.add(
                    egui::DragValue::new(&mut h)
                        .speed(2.0)
                        .range(16.0..=16384.0)
                        .max_decimals(0),
                );
                ui.add_space(8.0);
                page_size_presets_ui(ui, &mut w, &mut h, None);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    state.size_popup = if open && !cancel { Some((w, h)) } else { None };
    if !apply {
        return;
    }
    state.size_popup = None;
    let original = state
        .doc
        .page()
        .map(|p| (p.width, p.height))
        .unwrap_or((w, h));
    let (w, h) = (w.max(16.0), h.max(16.0));
    if (w, h) == original {
        return;
    }
    if let Ok(page) = state.doc.page_mut() {
        page.width = w;
        page.height = h;
    }
    let mut commands: Vec<Box<dyn canvas_core::Command>> = vec![Box::new(SetPageSize {
        before: original,
        after: (w, h),
    })];
    if let Some(cmd) = state.resync_background_cover() {
        commands.push(cmd);
    }
    state.push_undo_step(Box::new(canvas_core::Composite::new(
        "Resize page",
        commands,
    )));
}
