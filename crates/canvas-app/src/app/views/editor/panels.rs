//! Montaje de los paneles del frame del editor: la tira de lienzos de la
//! baraja, el panel de capas, el de propiedades y el area central con el
//! lienzo. Devuelve las acciones que hay que resolver despues, una vez
//! liberados los prestamos de los paneles.

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::{deck, deck_strip, editor, layers_panel};

use super::super::super::frame::EditorFrame;

pub(super) fn show_panels(
    state: &mut editor::EditorState,
    ui: &mut egui::Ui,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
) -> (
    Option<deck_strip::StripAction>,
    Option<editor::CanvasAction>,
) {
    // Tira de lienzos de la baraja: solo con más de un archivo en la
    // carpeta de origen. Va antes que "layers" para quedar pegada al borde
    // exterior de la ventana.
    let mut strip_action = None;
    // Acción pedida desde la cabecera de un lienzo del área central
    // (renombrar/duplicar/borrar) — se llena dentro del `CentralPanel` de
    // más abajo, se resuelve junto a `strip_action`.
    let mut canvas_action = None;
    if f.deck.is_visible() && !state.isolate {
        let active_dirty = state.is_dirty();
        // Ids DISTINTOS por lado (no el mismo panel reetiquetado): así el
        // tamaño recordado de la tira a la izquierda (ancho) no se aplica
        // como alto al moverla arriba, y viceversa — mismo criterio que ya
        // separa "layers" de "properties". `.resizable(true)` es
        // obligatorio en Top/Bottom (egui los crea con `resizable(false)`
        // por defecto) e inofensivo-pero-explícito en Left/Right. Orden
        // importa: `.default_size` ENSANCHA el rango si se llama después
        // de `.size_range`, así que va primero.
        match f.deck.strip_side {
            deck::StripSide::Left => {
                egui::Panel::left("deck_strip_left")
                    .default_size(120.0)
                    .size_range(96.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(f.deck, active_dirty, ui);
                    });
            }
            deck::StripSide::Right => {
                egui::Panel::right("deck_strip_right")
                    .default_size(120.0)
                    .size_range(96.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(f.deck, active_dirty, ui);
                    });
            }
            deck::StripSide::Top => {
                egui::Panel::top("deck_strip_top")
                    .default_size(140.0)
                    .size_range(120.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(f.deck, active_dirty, ui);
                    });
            }
            deck::StripSide::Bottom => {
                egui::Panel::bottom("deck_strip_bottom")
                    .default_size(140.0)
                    .size_range(120.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(f.deck, active_dirty, ui);
                    });
            }
        }
    }
    // Diseño bloqueado (`Slot::locked`, cabecera del lienzo en el área
    // central): deshabilita también los paneles, no solo los gestos sobre
    // el propio lienzo — "no se puede editar" sin matizar por qué vía.
    let locked = f.deck.slots.get(f.deck.active).is_some_and(|s| s.locked);
    egui::Panel::left("layers")
        .default_size(220.0)
        .show(ui, |ui| {
            ui.add_enabled_ui(!locked, |ui| layers_panel::layers_panel_ui(state, ui));
        });
    egui::Panel::right("properties")
        .default_size(260.0)
        .show(ui, |ui| {
            ui.add_enabled_ui(!locked, |ui| editor::properties_ui(state, ui));
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            canvas_action = editor::canvas_ui(
                state,
                f.deck,
                ui,
                rs,
                f.renderer,
                f.surface,
                f.tx,
                f.settings.new_canvas_format.extension(),
                f.settings.sidecar_default,
            );
        });

    (strip_action, canvas_action)
}
