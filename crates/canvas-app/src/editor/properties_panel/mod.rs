//! El panel de propiedades: archivo, página, insertar, y todos los
//! controles de la capa seleccionada (transform, blur, color, sombra,
//! contenido de texto/forma) — la UI que vive en el panel lateral derecho,
//! sin nada del lienzo en sí.
//!
//! Dividido por responsabilidad: `page` (resolución de página + ventanita
//! "Size"), `effects` (blur/color/sombra), `layer_common` (posición/tamaño/
//! recorte/alineación, comunes a cualquier capa) y `content`/`content_text`/
//! `content_shape` (controles propios de texto o forma).

mod content;
mod content_shape;
mod content_text;
mod effects;
mod layer_common;
mod page;

use canvas_core::LayerContent;
use eframe::egui;

use super::EditorState;
use crate::sidebar;

pub(in crate::editor) use page::size_popup_ui;

use layer_common::layer_properties_ui;
use page::page_ui;

/// Panel derecho: propiedades de la capa seleccionada.
pub fn properties_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    sidebar::compact(ui);
    sidebar::title(ui, "Properties");
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            properties_ui_inner(state, ui);
        });
}

/// Consolida cualquier edición de panel a medias (`panel_edit`/`blur_edit`/
/// `color_edit`/`content_edit`/`shadow_edit`) cuya capa ya no es la
/// seleccionada. Esos campos solo se limpian solos cuando el control que los
/// arma detecta `lost_focus()`/`drag_stopped()` — pero ese control solo se
/// dibuja mientras su capa sigue siendo `selection.primary()`
/// (`layer_properties_ui`/`content_properties_ui`). Cambiar de capa (o
/// deseleccionar) a mitad de una edición hace que ese control desaparezca
/// del árbol de UI sin soltar nunca el foco, dejando el campo pegado en
/// `Some(...)` para siempre: la edición se pierde como paso de deshacer y,
/// para `content_edit`, además bloquea Ctrl+Z/Ctrl+Y de TODO el editor
/// (`handle_shortcuts` se los cede a un `TextEdit` con foco propio mientras
/// `content_edit.is_some()`).
fn commit_stale_panel_edits(state: &mut EditorState) {
    let current = state.selection.primary();

    if matches!(&state.panel_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.panel_edit.take() {
            if let Ok(l) = state.doc.layer(id) {
                let after = l.transform;
                if after != before {
                    state.push_undo_step(Box::new(canvas_core::SetTransform {
                        layer: id,
                        before,
                        after,
                    }));
                }
            }
        }
    }
    if matches!(&state.blur_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.blur_edit.take() {
            let after = state
                .doc
                .layer(id)
                .map(|l| l.effects.blur_radius)
                .unwrap_or(before);
            if (after - before).abs() > f32::EPSILON {
                state.push_undo_step(Box::new(canvas_core::SetBlur {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
    if matches!(&state.color_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.color_edit.take() {
            let after = state.doc.layer(id).map(|l| l.effects).unwrap_or(before);
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetEffects {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
    if matches!(&state.content_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.content_edit.take() {
            let after = state
                .doc
                .layer(id)
                .map(|l| l.content.clone())
                .unwrap_or_else(|_| before.clone());
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetContent {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
    if matches!(&state.shadow_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.shadow_edit.take() {
            let after = state.doc.layer(id).ok().and_then(|l| l.effects.shadow);
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetShadow {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
}

fn properties_ui_inner(state: &mut EditorState, ui: &mut egui::Ui) {
    commit_stale_panel_edits(state);
    ui.add_space(8.0);

    // Banner: el archivo cambió en disco fuera de la app.
    if state.external_change {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "⚠ This file changed on disk outside Canvas Desktop.",
        );
        ui.horizontal(|ui| {
            if ui.button("Reload").clicked() {
                state.reload_requested = true;
            }
            if ui.button("Keep mine").clicked() {
                state.external_change = false;
            }
        });
        ui.separator();
    }

    if state.from_gallery.is_some() && ui.button("⏴ Back to gallery").clicked() {
        state.return_requested = true;
    }
    file_name_ui(state, ui);
    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (0.0, 0.0),
    };
    ui.weak(format!(
        "{} × {} px",
        page_dims.0 as i64, page_dims.1 as i64
    ));
    ui.separator();

    sidebar::section(ui, "Page", true, |ui| {
        page_ui(state, ui);
    });

    sidebar::section(ui, "Insert", false, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button("T Text").clicked() {
                state.insert_layer_centered(
                    "Text",
                    500.0,
                    120.0,
                    LayerContent::Text(canvas_core::TextContent::default()),
                );
            }
            if ui.small_button("R").on_hover_text("Rectangle").clicked() {
                state.insert_layer_centered(
                    "Rectangle",
                    320.0,
                    220.0,
                    LayerContent::Shape(canvas_core::ShapeContent::default()),
                );
            }
            if ui.small_button("O").on_hover_text("Ellipse").clicked() {
                state.insert_layer_centered(
                    "Ellipse",
                    280.0,
                    280.0,
                    LayerContent::Shape(canvas_core::ShapeContent {
                        kind: canvas_core::ShapeKind::Ellipse,
                        ..Default::default()
                    }),
                );
            }
            if ui.small_button("L").on_hover_text("Line").clicked() {
                state.insert_layer_centered(
                    "Line",
                    400.0,
                    24.0,
                    LayerContent::Shape(canvas_core::ShapeContent {
                        kind: canvas_core::ShapeKind::Line,
                        stroke: [30, 30, 30, 255],
                        stroke_width: 6.0,
                        ..Default::default()
                    }),
                );
            }
        });
    });

    sidebar::section(ui, "Layer", true, |ui| {
        if let Some(sel) = state.selection.primary() {
            if state.doc.layer(sel).is_ok() {
                layer_properties_ui(state, ui, sel, page_dims);
            }
        } else {
            ui.weak("No layer selected.");
            ui.weak("Click the image to select it.");
        }
    });

    sidebar::section(ui, "File actions", false, |ui| {
        ui.horizontal_wrapped(|ui| {
            let dirty_mark = if state.is_dirty() { " •" } else { "" };
            if ui
                .add_enabled(
                    !state.saving,
                    egui::Button::new(format!("💾 Save{dirty_mark}")),
                )
                .clicked()
            {
                state.save_clicked = true;
            }
            if ui
                .add_enabled(!state.saving, egui::Button::new("Save as…"))
                .clicked()
            {
                state.save_as_clicked = true;
            }
            // Va a la Papelera de reciclaje (`trash::delete`), no borrado
            // permanente: recuperable si el usuario se equivoca, así que no
            // hace falta pedir confirmación aparte.
            if ui
                .add_enabled(
                    !state.saving,
                    egui::Button::new(
                        egui::RichText::new("Delete").color(egui::Color32::from_rgb(220, 70, 70)),
                    ),
                )
                .clicked()
            {
                state.delete_requested = true;
            }
        });
        if state.is_design {
            ui.weak("Design file (.canvas) — layers are always kept.");
        } else {
            ui.checkbox(&mut state.sidecar_enabled, "Editable sidecar (.canvas)")
                .on_hover_text(
                    "Writes a .canvas file next to the image so the layers stay \
                     editable when you reopen it. Turn it off if you don't want \
                     extra files in your folders.",
                );
        }
        if state.saving {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Saving…");
            });
        }
        if let Some(error) = state.save_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, &error);
                if ui.small_button("✕").clicked() {
                    state.save_error = None;
                }
            });
        }
    });
    ui.label(format!("Zoom: {:.0} %", state.viewport.zoom * 100.0));
    ui.weak("Wheel: pan · Shift+wheel: pan the other axis · Ctrl+wheel: zoom");
    ui.weak("Space/middle button: pan · Ctrl+0: fit");
    ui.weak("Ctrl+S: save · Ctrl+Shift+S: save as");
    ui.weak("Ctrl+C / Ctrl+V: copy layers, even between designs");
    ui.add_space(4.0);
    if ui.small_button("⚙ Settings").clicked() {
        state.settings_clicked = true;
    }
}

/// Nombre del archivo abierto, arriba del panel: un lápiz lo vuelve editable
/// in-place (mismo patrón que el renombrado de la galería —
/// `gallery::gallery_cell` — y el de capas — `rename_edit_ui` más abajo)
/// cuando el documento ya tiene archivo en disco. Un diseño nuevo sin
/// guardar (`source_path` en `None`) no ofrece el lápiz: no hay nada que
/// renombrar todavía.
fn file_name_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    let id = egui::Id::new("editor_file_rename");
    if state.file_rename_edit.is_some() {
        // Mismo patrón que `gallery::gallery_cell`: Escape cancela, perder
        // el foco confirma (comprobado antes que `lost_focus` para que el
        // propio Escape no dispare un commit).
        let mut cancel = false;
        let mut commit = false;
        if let Some(text) = state.file_rename_edit.as_mut() {
            let resp = ui.add(egui::TextEdit::singleline(text).id(id));
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            } else if resp.lost_focus() {
                commit = true;
            }
        }
        if cancel {
            state.file_rename_edit = None;
        } else if commit {
            if let Some(text) = state.file_rename_edit.take() {
                let new_stem = text.trim().to_owned();
                let original_stem = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !new_stem.is_empty() && new_stem != original_stem {
                    state.file_rename_requested = Some(new_stem);
                }
            }
        }
    } else {
        ui.horizontal(|ui| {
            ui.heading(state.file_name());
            if state.doc.source_path.is_some() && ui.small_button("✏").clicked() {
                let stem = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                state.file_rename_edit = Some(stem);
                ui.memory_mut(|m| m.request_focus(id));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use canvas_core::{ImageContent, ShapeContent, Transform};
    use canvas_io::LoadedImage;

    use super::super::{DeleteRecord, GlobalStep};
    use super::*;

    fn loaded_image(width: u32, height: u32) -> LoadedImage {
        LoadedImage {
            rgba: vec![0u8; (width * height * 4) as usize],
            width,
            height,
        }
    }

    #[test]
    fn switching_selection_mid_edit_commits_the_pending_change_instead_of_leaking_it() {
        let mut state = EditorState::new_blank(500.0, 500.0);
        state.insert_layer_centered(
            "Text",
            200.0,
            80.0,
            LayerContent::Text(canvas_core::TextContent::default()),
        );
        let text_id = state.selection.primary().unwrap();
        let original = state.doc.layer(text_id).unwrap().content.clone();

        // Simula una edición de texto a medias: el control ya mutó el
        // documento en vivo (como hace `content_properties_ui` en cada
        // tecla), pero el usuario no soltó el foco del `TextEdit` todavía.
        state.content_edit = Some((text_id, original.clone()));
        if let LayerContent::Text(t) = &mut state.doc.layer_mut(text_id).unwrap().content {
            t.text = "edited mid-flight".to_owned();
        }

        // Selecciona otra capa SIN soltar el foco antes: en la UI real esto
        // hace que `content_properties_ui` deje de dibujarse para `text_id`
        // y `lost_focus()` nunca vuelva a disparar para ese control.
        state.insert_layer_centered(
            "Shape",
            50.0,
            50.0,
            LayerContent::Shape(canvas_core::ShapeContent::default()),
        );
        assert_ne!(state.selection.primary(), Some(text_id));
        assert!(
            state.content_edit.is_some(),
            "sigue colgado hasta que se reconcilie"
        );

        commit_stale_panel_edits(&mut state);

        assert!(
            state.content_edit.is_none(),
            "la edición colgada debe consolidarse, no seguir viva"
        );

        // El commit tardío debe haber quedado como su propio paso de
        // deshacer: un solo Ctrl+Z basta para devolver el texto a su valor
        // original, sin tocar la capa "Shape".
        state.undo();
        let restored = state.doc.layer(text_id).unwrap().content.clone();
        assert_eq!(restored, original);
        assert!(state.doc.layer(state.selection.primary().unwrap()).is_ok());
    }

    #[test]
    fn pasting_into_an_empty_canvas_expands_and_adds_a_blurred_background() {
        let mut state = EditorState::new_blank(1080.0, 1080.0);
        state.add_image_layer("Pasted Image", None, loaded_image(540, 960));

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 2);

        // Fondo: al fondo de la pila, cubre la página entera y desenfocado.
        let bg = &page.layers[0];
        assert_eq!(bg.name, "Blurred background");
        assert_eq!(bg.effects.blur_radius, 50.0);
        assert!(bg.transform.width >= page.width - 1e-6);
        assert!(bg.transform.height >= page.height - 1e-6);
        assert_eq!(Some(bg.id), state.background_layer);

        // Imagen: se amplía hasta tocar el alto (9:16 en página 1:1), centrada.
        let fg = &page.layers[1];
        assert!((fg.transform.height - page.height).abs() < 1e-6);
        assert!(fg.transform.width < page.width);
        assert!((fg.transform.x - (page.width - fg.transform.width) / 2.0).abs() < 1e-6);
        assert_eq!(state.selection.primary(), Some(fg.id));

        // Un solo Ctrl+Z deja el lienzo vacío otra vez (imagen + fondo).
        state.undo();
        assert!(state.doc.page().unwrap().layers.is_empty());
        assert!(state.selection.primary().is_none());
    }

    #[test]
    fn pasting_a_square_image_into_a_matching_square_canvas_skips_the_background() {
        let mut state = EditorState::new_blank(1000.0, 1000.0);
        state.add_image_layer("Pasted Image", None, loaded_image(500, 500));

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 1);
        assert_eq!(state.background_layer, None);
        let fg = &page.layers[0];
        assert_eq!((fg.transform.width, fg.transform.height), (1000.0, 1000.0));
    }

    #[test]
    fn replacing_image_preserves_transform_and_undo_restores_the_old_layer() {
        let mut state = EditorState::new_blank(1000.0, 1000.0);
        state.add_image_layer("Original", None, loaded_image(500, 500));
        let original_layer = state.doc.page().unwrap().layers[0].clone();

        state
            .replace_image_layer(original_layer.id, None, loaded_image(250, 500))
            .unwrap();

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 1);
        let replaced = &page.layers[0];
        assert_ne!(replaced.id, original_layer.id);
        assert_eq!(replaced.name, original_layer.name);
        assert_eq!(replaced.transform, original_layer.transform);
        assert_eq!(state.selection.primary(), Some(replaced.id));
        match &replaced.content {
            LayerContent::Image(content) => {
                assert_eq!((content.natural_width, content.natural_height), (250, 500));
                assert_eq!(content.crop, None);
            }
            _ => panic!("replacement must stay an image layer"),
        }

        state.undo();

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 1);
        let restored = &page.layers[0];
        assert_eq!(restored.id, original_layer.id);
        assert_eq!(restored.transform, original_layer.transform);
        match &restored.content {
            LayerContent::Image(content) => {
                assert_eq!((content.natural_width, content.natural_height), (500, 500));
            }
            _ => panic!("undo must restore the original image layer"),
        }
        assert!(state.images.contains_key(&original_layer.id));
    }

    /// Deshacer un borrado de archivo real (`GlobalStep::Delete`, apilado
    /// vía `record_delete` tras un «Delete» del usuario) no pertenece a
    /// ninguna ranura: se resuelve de inmediato — dejando la restauración
    /// pedida en `pending_restore` — sin esperar ningún salto de baraja, y
    /// sin dejar rastro en `global_redo` (no se "rehace" volver a borrar).
    #[test]
    fn undoing_a_delete_step_requests_a_restore_without_touching_redo() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        state.pending_creation = false; // no es lo que se está probando aquí
        let record = DeleteRecord {
            path: PathBuf::from("C:/photos/cat.png"),
            sidecar: Some(PathBuf::from("C:/photos/.canvas/cat.png.canvas")),
        };
        state.record_delete(record.clone());
        assert!(state.can_undo());

        state.undo();

        assert_eq!(state.pending_restore, Some(record));
        assert!(!state.can_undo(), "el paso de borrado ya se resolvió");
        assert!(
            !state.can_redo(),
            "deshacer un borrado no deja nada que rehacer"
        );
    }

    /// El borrado que dispara `finish_pending_global_undo` al deshacer una
    /// creación (`GlobalStep::Create`) queda marcado como "no venía de un
    /// clic del usuario" (`pending_delete_from_undo`) — es la señal que lee
    /// `main.rs` para NO apilarle a su vez un `GlobalStep::Delete`: si lo
    /// hiciera, un `Ctrl+Z` más adelante podría "deshacer el deshacer" y
    /// restaurar un lienzo que el propio usuario decidió descartar.
    #[test]
    fn finishing_a_pending_create_undo_marks_the_delete_as_not_user_initiated() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        state.active_slot_id = 1;
        state.pending_creation = false;
        state.pending_global_undo = Some(GlobalStep::Create(1));

        assert!(!state.pending_delete_from_undo);
        state.finish_pending_global_undo();

        assert!(state.delete_requested);
        assert!(state.pending_delete_from_undo);
    }

    /// Un lienzo recién creado (`new_blank`) nace con `pending_creation`
    /// activo y SIN nada que deshacer todavía: hasta que no se edita de
    /// verdad, "crear" no es un paso — evita que un relleno automático de la
    /// baraja que nadie llega a tocar se cuele como un paso de deshacer
    /// fantasma (la causa del bug: registrar la creación en cada sitio
    /// donde podía aparecer una ranura, en vez de en su primera edición
    /// real, dejaba huecos y duplicados según la carrera entre clics del
    /// usuario y el relleno asíncrono).
    #[test]
    fn a_freshly_created_canvas_has_nothing_to_undo_until_its_first_edit() {
        let state = EditorState::new_blank(10.0, 10.0);
        assert!(state.pending_creation);
        assert!(!state.can_undo());
    }

    /// La primera edición real de un lienzo recién creado antepone su
    /// `GlobalStep::Create` en la pila global: dos `Ctrl+Z` deshacen esa
    /// UNA edición y LUEGO piden borrar la ranura entera — no las dos
    /// cosas de golpe con un solo `Ctrl+Z` (el bug reportado).
    #[test]
    fn first_edit_of_a_freshly_created_canvas_records_its_creation_too() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        let id = state
            .doc
            .add_layer(
                "a",
                Transform::new(0.0, 0.0, 1.0, 1.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 1,
                    natural_height: 1,
                    crop: None,
                }),
            )
            .unwrap();
        state.active_slot_id = 2;

        state.doc.layer_mut(id).unwrap().name = "b".to_string();
        state.push_undo_step(Box::new(canvas_core::Rename {
            layer: id,
            before: "a".to_string(),
            after: "b".to_string(),
        }));
        assert!(
            !state.pending_creation,
            "se consume en la primera edición real"
        );

        // Ctrl+Z #1: deshace SOLO la edición; la ranura sigue viva.
        state.undo();
        assert!(state.pending_global_undo.is_none());
        assert_eq!(state.doc.layer(id).unwrap().name, "a");
        assert!(!state.delete_requested);
        assert!(
            state.can_undo(),
            "todavía queda el paso de creación por deshacer"
        );

        // Ctrl+Z #2: ahora sí, pide borrar la ranura entera.
        state.undo();
        assert_eq!(state.pending_global_undo, Some(GlobalStep::Create(2)));
        state.finish_pending_global_undo();
        assert!(
            state.delete_requested,
            "debe pedir borrar la ranura recién creada"
        );
        assert!(
            !state.can_undo(),
            "crear no deja nada más que deshacer detrás"
        );
    }

    /// Simula "editar el diseño 1, luego el 3, luego el 1 otra vez" con
    /// `active_slot_id` (una sola `EditorState`/`Document` de sobra para
    /// probar el ORDEN cruzado — el salto real de baraja lo cubre
    /// `deck::apply_jump` por separado). Comprueba que deshacer tres veces
    /// reproduce el orden cronológico real (1, 3, 1) pidiendo el salto
    /// correspondiente cada vez que le toca a un diseño que no es el
    /// activo, y que rehacer reconstruye el mismo cruce en sentido inverso.
    #[test]
    fn global_undo_and_redo_replay_steps_in_true_chronological_order_across_designs() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        let id = state
            .doc
            .add_layer(
                "a",
                Transform::new(0.0, 0.0, 1.0, 1.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 1,
                    natural_height: 1,
                    crop: None,
                }),
            )
            .unwrap();
        // Este test es sobre el orden entre `Edit`, no sobre `Create` (ya
        // cubierto aparte) — se apaga para no mezclar ambos.
        state.pending_creation = false;
        let rename = |state: &mut EditorState, before: &str, after: &str| {
            state.doc.layer_mut(id).unwrap().name = after.to_string();
            state.push_undo_step(Box::new(canvas_core::Rename {
                layer: id,
                before: before.to_string(),
                after: after.to_string(),
            }));
        };
        let name = |state: &EditorState| state.doc.layer(id).unwrap().name.clone();

        state.active_slot_id = 1;
        rename(&mut state, "a", "b"); // diseño 1
        state.active_slot_id = 3;
        rename(&mut state, "b", "c"); // diseño 3
        state.active_slot_id = 1;
        rename(&mut state, "c", "d"); // diseño 1 otra vez

        // El paso más reciente es del diseño activo (1): deshace en el sitio.
        state.undo();
        assert!(state.pending_global_undo.is_none());
        assert_eq!(name(&state), "c");

        // El siguiente le toca al diseño 3: pide el salto SIN tocar el
        // documento todavía.
        state.undo();
        assert_eq!(state.pending_global_undo, Some(GlobalStep::Edit(3)));
        assert_eq!(name(&state), "c");

        // `main.rs` completó el salto de baraja al diseño 3.
        state.active_slot_id = 3;
        state.finish_pending_global_undo();
        assert_eq!(name(&state), "b");

        // Queda el primer paso, del diseño 1: pide saltar de vuelta.
        state.undo();
        assert_eq!(state.pending_global_undo, Some(GlobalStep::Edit(1)));
        state.active_slot_id = 1;
        state.finish_pending_global_undo();
        assert_eq!(name(&state), "a");
        assert!(!state.can_undo());

        // Rehacer reproduce el mismo cruce de diseños en sentido inverso.
        state.redo();
        assert_eq!(name(&state), "b");
        state.redo();
        assert_eq!(state.pending_global_redo, Some(GlobalStep::Edit(3)));
        state.active_slot_id = 3;
        state.finish_pending_global_redo();
        assert_eq!(name(&state), "c");
        state.redo();
        assert_eq!(state.pending_global_redo, Some(GlobalStep::Edit(1)));
        state.active_slot_id = 1;
        state.finish_pending_global_redo();
        assert_eq!(name(&state), "d");
        assert!(!state.can_redo());
    }

    #[test]
    fn pasting_into_a_non_empty_canvas_keeps_the_old_contain_behavior() {
        let mut state = EditorState::new_blank(1080.0, 1080.0);
        // Deja el lienzo no vacío con una capa que no es una imagen.
        state.insert_layer_centered(
            "Rect",
            100.0,
            100.0,
            LayerContent::Shape(ShapeContent::default()),
        );

        state.add_image_layer("Pasted Image", None, loaded_image(540, 960));

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 2);
        assert_eq!(state.background_layer, None);

        // Nunca se amplía sobre un lienzo no vacío: 960 < 1080, cabe sin
        // escalar, y "contain" no se aplica (comportamiento de siempre).
        let fg = &page.layers[1];
        assert_eq!((fg.transform.width, fg.transform.height), (540.0, 960.0));
    }
}
