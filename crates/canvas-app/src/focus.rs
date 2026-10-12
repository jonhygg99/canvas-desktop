//! Foco diferido de un frame para cuadros de texto recién abiertos.
//!
//! `egui::Memory::request_focus` sobre un `Id` cuyo widget aún no existe en
//! el frame actual deja el foco apuntando a un nodo que accesskit no conoce,
//! y en Windows eso aborta el proceso dentro de
//! `accesskit_consumer::State::update` («Focused ID … is not in the node
//! list»). Pasaba al pulsar «New folder» y en todos los renombrados
//! in-place (carpeta, celda, capa, hueco del deck, nombre de archivo): el
//! clic pedía el foco para un `TextEdit` que solo existe a partir del frame
//! siguiente.
//!
//! El patrón seguro es en dos tiempos: al abrir la edición se arma
//! (`arm_focus_once`) y al pintar el widget se consume (`focus_once`) sobre
//! la `Response` viva, una sola vez — así el foco nunca apunta a un `Id`
//! sin nodo y no se le roba al usuario en los frames siguientes.

use eframe::egui;

fn flag_id(id: egui::Id) -> egui::Id {
    id.with("__focus_once_armed")
}

/// Arma el foco para el widget con `id`: la próxima vez que se pinte y se
/// pase por `focus_once`, quedará enfocado. Llamarlo solo no toca el foco,
/// así que es seguro aunque el widget tarde un frame en existir.
pub(crate) fn arm_focus_once(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.insert_temp(flag_id(id), true));
}

/// Enfoca `response` si (y solo si) su `id` se armó con `arm_focus_once`.
/// Debe llamarse justo después de crear el widget, con su `Response` viva.
/// Consume la marca: los frames siguientes no roban el foco.
pub(crate) fn focus_once(ui: &egui::Ui, id: egui::Id, response: &egui::Response) {
    // `remove_temp` consume la marca en el mismo paso: no queda un `false`
    // eterno por cada campo enfocado alguna vez.
    let armed: bool = ui
        .data_mut(|data| data.remove_temp(flag_id(id)))
        .unwrap_or(false);
    if armed {
        response.request_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        }
    }

    fn run_frame(ctx: &egui::Context, add_ui: impl FnMut(&mut egui::Ui)) {
        let _ = ctx.run_ui(screen_input(), add_ui);
    }

    fn focused(ctx: &egui::Context) -> Option<egui::Id> {
        ctx.memory(|m| m.focused())
    }

    #[test]
    fn armar_sin_widget_no_toca_el_foco() {
        // Regresión del crash de «New folder»: el `request_focus` directo
        // dejaba el foco en un `Id` sin nodo de accesskit. Armar sin pintar
        // el widget no debe enfocar nada.
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new("focus_once_regression");
        arm_focus_once(&ctx, id);
        run_frame(&ctx, |_| {});
        assert_eq!(focused(&ctx), None);
    }

    #[test]
    fn enfoca_al_aparecer_y_no_roba_despues() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new("focus_once_widget");
        let other = egui::Id::new("focus_once_other");
        let mut text = String::new();

        // Frame 1: el widget aparece y se lleva el foco una vez.
        arm_focus_once(&ctx, id);
        run_frame(&ctx, |ui| {
            let response = ui.add(egui::TextEdit::singleline(&mut text).id(id));
            focus_once(ui, id, &response);
        });
        assert_eq!(focused(&ctx), Some(id));

        // Frame 2: el foco está en otro widget vivo y `focus_once` no lo roba.
        run_frame(&ctx, |ui| {
            let mut other_text = String::new();
            let other_response = ui.add(egui::TextEdit::singleline(&mut other_text).id(other));
            other_response.request_focus();
            let response = ui.add(egui::TextEdit::singleline(&mut text).id(id));
            focus_once(ui, id, &response);
        });
        assert_eq!(focused(&ctx), Some(other));
    }

    #[test]
    fn rearmar_vuelve_a_enfocar() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let id = egui::Id::new("focus_once_rearm");
        let mut text = String::new();

        for _ in 0..2 {
            arm_focus_once(&ctx, id);
            run_frame(&ctx, |ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut text).id(id));
                focus_once(ui, id, &response);
            });
            assert_eq!(focused(&ctx), Some(id));
            // Simula el cierre de la edición: el foco se va a otra parte.
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("elsewhere")));
        }
    }
}
