//! Historial visible del diseño activo; los saltos mantienen el diario global.
use super::EditorState;
use eframe::egui;

pub(crate) fn show(state: &mut EditorState, ui: &mut egui::Ui) {
    ui.menu_button(crate::i18n::tr("History"), |ui| {
        ui.set_min_width(240.0);
        let current = state.history.undo_depth();
        let range = state.history_range();
        let enabled = state.is_idle()
            && state.framing.is_none()
            && state.pending_global_undo.is_none()
            && state.pending_global_redo.is_none();
        let mut entries = vec![(0, crate::i18n::tr("Earlier state").to_owned())];
        entries.extend(
            state
                .history
                .undo_labels()
                .enumerate()
                .map(|(i, label)| (i + 1, action_label(label))),
        );
        entries.extend(
            state
                .history
                .redo_labels()
                .enumerate()
                .map(|(i, label)| (current + i + 1, action_label(label))),
        );
        let mut target = None;
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for (depth, label) in entries.into_iter().rev() {
                    let text = format!("{depth} · {label}");
                    if ui
                        .add_enabled(
                            enabled && range.contains(&depth),
                            egui::Button::selectable(depth == current, text),
                        )
                        .clicked()
                    {
                        target = Some(depth);
                    }
                }
            });
        ui.weak(crate::i18n::tr("Choose a state to undo or redo"));
        if *range.start() > 0 {
            ui.weak(crate::i18n::tr("Earlier steps belong to another canvas"));
        }
        if let Some(target) = target {
            state.jump_history(target);
            ui.close();
        }
    });
}

fn action_label(label: &str) -> String {
    // Los comandos antiguos tienen etiquetas internas en español.
    let english = match label {
        "Transformar" | "Transformar capa" | "Mover selección" | "Transformar selección" => {
            "Transform selection"
        }
        "Contenido" | "Editar contenido" => "Edit content",
        "Desenfoque" => "Blur",
        "Sombra" => "Shadow",
        "Color" | "Ajustes de color" | "Ajustes" => "Color",
        "Opacidad" => "Opacity",
        "Visibilidad" | "Mostrar/ocultar capa" => "Visibility",
        "Bloquear" | "Bloqueo" | "Bloquear capa" => "Lock",
        "Renombrar" | "Renombrar capa" => "Rename",
        "Insertar capa" | "Añadir capa" => "Insert layer",
        "Eliminar capa" | "Quitar capa" | "Quitar capas" => "Delete layer",
        "Reordenar" | "Reordenar capas" => "Reorder layers",
        "Agrupar" => "Group layers",
        "Desagrupar" => "Ungroup layers",
        "Recortar" | "Recorte" => "Crop",
        "Tamaño de página" | "Cambiar resolución" => "Page size",
        "Alinear selección" => "Align selection",
        "Distribuir selección" => "Distribute selection",
        "Organizar selección" => "Arrange selection",
        "Pegar capas" => "Paste",
        other => other,
    };
    crate::i18n::tr(english).to_owned()
}
