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
    let layers_collapsed_before = f.settings.layers_collapsed;
    // Panel de capas con animación manual del ancho: un único panel
    // cuyo `exact_size` se interpola entre 36 px (colapsado) y 220 px
    // (expandido) con ease-out cúbico de 0,2 s, igual que el
    // intercambio de pestañas. El estado de la animación vive en el
    // `data` de egui; el contenido se elige por umbral de ancho. Así
    // hay un solo panel, una sola rama, y el clic que cambia
    // `layers_collapsed` no puede rebotar porque la otra vista jamás
    // se ejecuta en el mismo frame.
    const COLLAPSED_WIDTH: f32 = 36.0;
    const EXPANDED_WIDTH: f32 = 220.0;
    const PANEL_ANIM_SECS: f64 = 0.2;

    let anim_salt = egui::Id::new("layers_panel_anim");
    type AnimState = (bool, f64, bool); // (target_collapsed, start_time, started)
    let mut anim: Option<AnimState> = ui.data_mut(|d| d.get_temp(anim_salt).unwrap_or(None));
    let now: f64 = ui.input(|i| i.time);
    let target_collapsed = f.settings.layers_collapsed;

    // Si el objetivo cambió, arrancar (o reiniciar) la animación.
    if anim.map_or(true, |(t, _, _)| t != target_collapsed) {
        anim = Some((target_collapsed, now, false));
    }

    let mut width = if target_collapsed { COLLAPSED_WIDTH } else { EXPANDED_WIDTH };
    if let Some((target, start, started)) = &mut anim {
        if !*started {
            *start = now;
            *started = true;
        }
        let elapsed = now - *start;
        if elapsed >= PANEL_ANIM_SECS {
            // Animación terminada: NO ponemos `anim = None` porque en el
            // frame siguiente `None` se interpretaría como «arranca una
            // animación nueva» y el panel rebotaría sin fin.
        } else {
            let t = (elapsed / PANEL_ANIM_SECS) as f32;
            let ease = 1.0 - (1.0 - t).powi(3); // ease-out cúbico
            if *target {
                width = EXPANDED_WIDTH + (COLLAPSED_WIDTH - EXPANDED_WIDTH) * ease;
            } else {
                width = COLLAPSED_WIDTH + (EXPANDED_WIDTH - COLLAPSED_WIDTH) * ease;
            }
            ui.ctx().request_repaint();
        }
    }
    ui.data_mut(|d| d.insert_temp(anim_salt, anim));

    // Un solo panel, ancho animado. Por debajo de la mitad del
    // recorrido se muestra la tira de iconos; por encima, el panel
    // completo con título y elementos.
    let midpoint = (COLLAPSED_WIDTH + EXPANDED_WIDTH) * 0.5;
    egui::Panel::left("layers")
        .exact_size(width)
        .resizable(false)
        .show(ui, |ui| {
            if width < midpoint {
                let new_order = layers_panel::vertical_tab_strip_ui(
                    ui,
                    &mut state.active_left_tab,
                    &mut f.settings.layers_collapsed,
                    f.settings.layers_tab_order,
                    true,
                );
                if let Some(new_order) = new_order {
                    if f.settings.layers_tab_order != new_order {
                        f.settings.layers_tab_order = new_order;
                        f.settings.save_in_background();
                    }
                }
            } else {
                ui.add_enabled_ui(!locked, |ui| {
                    let new_order = layers_panel::left_panel_ui(
                        state,
                        ui,
                        &mut f.settings.layers_collapsed,
                        f.settings.layers_tab_order,
                    );
                    if let Some(new_order) = new_order {
                        if f.settings.layers_tab_order != new_order {
                            f.settings.layers_tab_order = new_order;
                            f.settings.save_in_background();
                        }
                    }
                });
            }
        });
    if f.settings.layers_collapsed != layers_collapsed_before {
        f.settings.save_in_background();
    }
    egui::Panel::right("properties")
        .default_size(260.0)
        .show(ui, |ui| {
            ui.add_enabled_ui(!locked, |ui| editor::properties_ui(state, ui));
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            let mut ctx = editor::canvas::CanvasContext {
                rs,
                renderer: f.renderer,
                surface: f.surface,
                tx: f.tx,
                new_canvas_ext: f.settings.new_canvas_format.extension(),
                sidecar_default: f.settings.sidecar_default,
            };
            canvas_action = editor::canvas_ui(state, f.deck, ui, &mut ctx);
        });

    (strip_action, canvas_action)
}
