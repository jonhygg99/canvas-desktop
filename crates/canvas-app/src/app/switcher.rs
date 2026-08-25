//! Conmutador rápido de workspaces: `Ctrl+Tab`/`Ctrl+Shift+Tab` ciclan
//! entre ventanas al instante (con una paleta breve encima), y ``Ctrl+` ``
//! abre la paleta completa para saltar con las flechas y Enter.
//!
//! El estado vive en `AppInner` (compartido entre ventanas): cada frame, la
//! ventana ENFOCADA procesa las teclas y dibuja la paleta, y pedir el foco
//! para otra ventana es un `ViewportCommand::Focus` enviado a su
//! `ViewportId` — no hay más magia.

use eframe::egui;

use crate::app::Workspace;

/// Estado del conmutador, compartido entre todas las ventanas.
#[derive(Default)]
pub(crate) struct SwitcherState {
    /// La paleta está abierta.
    pub open: bool,
    /// Índice seleccionado dentro de `AppInner::workspaces`.
    pub selected: usize,
    /// Última pulsación de Ctrl+Tab (`ui.input(|i| i.time)`), para el
    /// overlay efímero que acompaña al ciclo inmediato.
    pub last_cycle: Option<f64>,
}

impl SwitcherState {
    /// Teclas globales del conmutador. Se llama desde el frame de la ventana
    /// enfocada (solo esa ve sus propias teclas). Devuelve `Some(idx)`
    /// cuando hay que ENFOCAR el workspace `idx` (ciclo inmediato o Enter
    /// de la paleta).
    pub fn handle_keys(
        &mut self,
        ctx: &egui::Context,
        ws_count: usize,
        focused: usize,
        now: f64,
    ) -> Option<usize> {
        if ws_count == 0 {
            return None;
        }
        let ctrl_tab = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Tab));
        let ctrl_shift_tab = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Tab,
            )
        });
        let ctrl_backquote =
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backtick));
        let enter = ctx.input_mut(|i| i.key_pressed(egui::Key::Enter));
        let escape = ctx.input_mut(|i| i.key_pressed(egui::Key::Escape));

        if ctrl_tab || ctrl_shift_tab {
            self.open = true;
            let next = if ctrl_shift_tab {
                (self.selected + ws_count - 1) % ws_count
            } else {
                (self.selected + 1) % ws_count
            };
            self.selected = next;
            self.last_cycle = Some(now);
            return Some(next);
        }
        if ctrl_backquote {
            self.open = !self.open;
            self.selected = if self.open { focused } else { self.selected };
            return None;
        }
        if !self.open {
            return None;
        }
        if escape {
            self.open = false;
            return None;
        }
        if ctx.input_mut(|i| {
            i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown)
        }) {
            self.selected = (self.selected + 1) % ws_count;
        }
        if ctx
            .input_mut(|i| i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp))
        {
            self.selected = (self.selected + ws_count - 1) % ws_count;
        }
        if enter {
            self.open = false;
            return Some(self.selected);
        }
        None
    }
}

/// Dibuja la paleta del conmutador EN LA VENTANA ENFOCADA (el llamador solo
/// la invoca para el workspace enfocado). Devuelve el índice del workspace a
/// enfocar si el usuario hizo clic en una fila.
///
/// `self_idx` es el índice del workspace del frame actual y `self_ws` su
/// referencia: su lock ya está tomado por el frame, así que sus datos se
/// leen de la referencia (los demás se lockean por fila; `std::sync::Mutex`
/// no es reentrante).
pub(super) fn switcher_overlay(
    ctx: &egui::Context,
    state: &mut SwitcherState,
    workspaces: &[std::sync::Arc<std::sync::Mutex<Workspace>>],
    self_idx: usize,
    self_ws: &Workspace,
) -> Option<usize> {
    if !state.open || workspaces.is_empty() {
        state.open = false;
        return None;
    }
    state.selected = state.selected.min(workspaces.len() - 1);

    // Snapshots delante del cierre: leer las etiquetas con el mutex
    // tomado por fila, sin retener el lock dentro del `show`. El workspace
    // del frame actual ya está lockeado por su `&mut` — se lee de ahí.
    let mut rows: Vec<(String, bool)> = Vec::with_capacity(workspaces.len());
    for (i, ws_arc) in workspaces.iter().enumerate() {
        if i == self_idx {
            rows.push((self_ws.label(), self_ws.is_dirty()));
        } else {
            let ws = ws_arc.lock().unwrap();
            rows.push((ws.label(), ws.is_dirty()));
        }
    }

    let viewport_rect = ctx.viewport_rect();
    let mut focus_request = None;
    egui::Area::new(egui::Id::new("workspace_switcher"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0))
        .order(egui::Order::Foreground)
        .constrain_to(viewport_rect)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.label(egui::RichText::new("Switch workspace").strong());
            ui.separator();
            for (i, (name, dirty)) in rows.iter().enumerate() {
                let text = if *dirty {
                    format!("* {name}")
                } else {
                    name.clone()
                };
                if ui.selectable_label(i == state.selected, text).clicked() {
                    state.selected = i;
                    state.open = false;
                    focus_request = Some(i);
                }
            }
            ui.weak(
                "Ctrl+N new window  ·  Ctrl+T open folder…  ·  Ctrl+Tab cycles  ·  Enter switch",
            );
        });
    focus_request
}
