//! El lienzo activo: menú contextual, render (GPU vello + chrome de los
//! lienzos vecinos), hit-testing de la baraja/cabeceras, y el popup de
//! "Replace from URL" — el orquestador que junta viewport, interacción,
//! overlays y el chrome de la tira en una sola superficie de `canvas_ui`.

use std::sync::mpsc::Sender;

use canvas_core::{Document, LayerContent, LayerId, Transform};
use canvas_render::{CanvasRenderer, FxScope, ImageMap};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::kurbo::Affine;

use crate::deck::{Deck, DeckAxis, DeckRect, MoveDir, SlotContent};
use crate::loader::{self, AppMsg};
use crate::surface::CanvasSurface;

use super::interaction::layer_interaction;
use super::layer_ops::{apply_alignment, reorder_layer, sibling_position, ZOrder};
use super::overlay::{draw_grid, draw_rulers, draw_selection_overlay};
use super::properties_panel::size_popup_ui;
use super::slot_chrome::{
    draw_add_zone, draw_header_tooltips, draw_rename_overlay, draw_slot_chrome, slot_header_layout,
};
use super::viewport::{page_to_screen, screen_to_page, AutoFit};
use super::EditorState;

/// Acción pedida desde la cabecera de un lienzo (área central) que necesita
/// tocar disco (duplicar/borrar) o reconciliarse con el nombre real del
/// archivo (renombrar) — `canvas_ui` la arma pero no la ejecuta; se resuelve
/// en `main.rs`, mismo espíritu que `StripAction` desde la tira.
pub enum CanvasAction {
    Rename(u64, String),
    Duplicate(u64),
    Delete(u64),
    ReplaceFromLocal(LayerId),
    ReplaceFromUrl(LayerId, String),
    /// Elegido en el menú contextual (clic derecho) del propio lienzo —
    /// reutiliza el mismo `MenuAction` que ya resuelve la barra de menú
    /// nativa/de respaldo, sin duplicar esa lógica.
    Menu(crate::menus::MenuAction),
}

fn replace_url_popup_ui(state: &mut EditorState, ctx: &egui::Context) -> Option<CanvasAction> {
    let (layer, mut url) = state.replace_url_popup.take()?;
    let mut open = true;
    let mut replace = false;
    let mut cancel = false;
    egui::Window::new("Replace from URL")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut url)
                    .hint_text("https://example.com/image.jpg")
                    .desired_width(360.0),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!url.trim().is_empty(), egui::Button::new("Replace"))
                    .clicked()
                {
                    replace = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if replace {
        Some(CanvasAction::ReplaceFromUrl(layer, url.trim().to_owned()))
    } else {
        if open && !cancel {
            state.replace_url_popup = Some((layer, url));
        }
        None
    }
}
/// El lienzo: gestiona zoom/paneo, carga perezosa/descarte de la baraja, y
/// renderiza en una sola escena todos los lienzos visibles (el activo con
/// `state.doc`/`state.images`; el resto con su propio `SlotDoc`).
#[allow(clippy::too_many_arguments)]
pub fn canvas_ui(
    state: &mut EditorState,
    deck: &mut Deck,
    ui: &mut egui::Ui,
    rs: &RenderState,
    renderer: &mut CanvasRenderer,
    surface_slot: &mut Option<CanvasSurface>,
    tx: &Sender<AppMsg>,
    // Extensión de `settings.new_canvas_format` — qué crea la zona "+" al
    // final de la baraja cuando se pulsa directamente sobre el lienzo.
    new_canvas_ext: &str,
) -> Option<CanvasAction> {
    // Duplicar/borrar/renombrar tocan disco o el watcher: se arman aquí (en
    // la cabecera de un lienzo, ver más abajo) pero se resuelven en
    // `main.rs`, igual que `StripAction` desde la tira.
    let mut action: Option<CanvasAction> = None;

    let avail = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

    // Menú contextual (clic derecho): antes no había ninguno en el área de
    // edición. Solo las acciones que de verdad se usan desde un clic
    // derecho — no una copia entera del menú Edit (eso ya está a un atajo
    // de teclado o al menú superior de distancia).
    response.context_menu(|ui| {
        use crate::menus::MenuAction;
        let mut item = |ui: &mut egui::Ui, label: &str, enabled: bool, a: MenuAction| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                action = Some(CanvasAction::Menu(a));
                ui.close();
            }
        };
        item(ui, "Undo", state.can_undo(), MenuAction::Undo);
        item(ui, "Redo", state.can_redo(), MenuAction::Redo);
        ui.separator();
        item(ui, "Cut", true, MenuAction::Cut);
        item(ui, "Copy", true, MenuAction::Copy);
        item(ui, "Paste", true, MenuAction::Paste);
        item(ui, "Duplicate", true, MenuAction::Duplicate);
        item(ui, "Delete", true, MenuAction::Delete);
        ui.separator();
        item(ui, "Select All", true, MenuAction::SelectAll);
        item(ui, "Group", true, MenuAction::Group);
        item(ui, "Ungroup", true, MenuAction::Ungroup);
        let selected_image = state.selection.primary().filter(|id| {
            state
                .doc
                .layer(*id)
                .ok()
                .is_some_and(|l| matches!(l.content, LayerContent::Image(_)))
        });
        let design_sources: Vec<(LayerId, String)> = selected_image
            .and_then(|target| {
                state.doc.page().ok().map(|page| {
                    page.layers
                        .iter()
                        .filter(|layer| {
                            layer.id != target
                                && matches!(layer.content, LayerContent::Image(_))
                                && state.images.contains_key(&layer.id)
                        })
                        .map(|layer| (layer.id, layer.name.clone()))
                        .collect()
                })
            })
            .unwrap_or_default();
        ui.add_enabled_ui(selected_image.is_some(), |ui| {
            ui.menu_button("Replace", |ui| {
                let Some(target) = selected_image else {
                    return;
                };

                ui.menu_button("From this design", |ui| {
                    if design_sources.is_empty() {
                        ui.add_enabled(false, egui::Button::new("No other images"));
                    }
                    for (source, name) in &design_sources {
                        if ui.button(name).clicked() {
                            if let Err(e) = state.replace_image_from_layer(target, *source) {
                                state.save_error = Some(e);
                            }
                            ui.close();
                        }
                    }
                });

                if ui.button("From local file").clicked() {
                    action = Some(CanvasAction::ReplaceFromLocal(target));
                    ui.close();
                }
                if ui.button("From internet URL").clicked() {
                    state.replace_url_popup = Some((target, String::new()));
                    ui.close();
                }
            });
        });
        ui.separator();
        // Orden y alineación de la capa PRIMARIA seleccionada — deshabilitados
        // enteros (el propio botón del submenú) sin selección, en vez de
        // mostrar el submenú vacío o con todo gris dentro.
        let sel = state.selection.primary();
        ui.add_enabled_ui(sel.is_some(), |ui| {
            ui.menu_button("Layers", |ui| {
                let Some(id) = sel else {
                    return;
                };
                // Bring to Front/Move Forward no tendrían efecto si ya está
                // en el extremo del frente (`current == last`); Move
                // Backward/Send to Back igual en el del fondo (`current ==
                // 0`) — se deshabilitan en vez de dejarlos ahí sin más,
                // para que el diseño lo demuestre en vez de solo no-opear.
                let range = sibling_position(state, id);
                let can_go_forward = range.is_some_and(|(_, current, last)| current < last);
                let can_go_backward = range.is_some_and(|(_, current, _)| current > 0);
                let mut z = |ui: &mut egui::Ui, label: &str, enabled: bool, to: ZOrder| {
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        reorder_layer(state, id, to);
                        ui.close();
                    }
                };
                z(ui, "Bring to Front", can_go_forward, ZOrder::Front);
                z(ui, "Move Forward", can_go_forward, ZOrder::Forward);
                z(ui, "Move Backward", can_go_backward, ZOrder::Backward);
                z(ui, "Send to Back", can_go_backward, ZOrder::Back);
            });
        });
        ui.add_enabled_ui(sel.is_some(), |ui| {
            ui.menu_button("Align to Page", |ui| {
                let Some(id) = sel else {
                    return;
                };
                let Ok(page) = state.doc.page() else {
                    return;
                };
                let (page_w, page_h) = (page.width, page.height);
                let Some(t) = state.doc.layer(id).ok().map(|l| l.transform) else {
                    return;
                };
                // `selectable_label`, no `button`: resalta la opción que YA
                // coincide con la posición actual de la capa — mismo widget
                // que ya usa este archivo para "elegido entre varias
                // opciones" (alineación de texto, más abajo en este mismo
                // módulo).
                let mut a = |ui: &mut egui::Ui, label: &str, after: Transform| {
                    if ui.selectable_label(after == t, label).clicked() {
                        apply_alignment(state, id, after);
                        ui.close();
                    }
                };
                a(
                    ui,
                    "Left",
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Left),
                );
                a(
                    ui,
                    "Center",
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center),
                );
                a(
                    ui,
                    "Right",
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Right),
                );
                ui.separator();
                a(
                    ui,
                    "Top",
                    canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Top),
                );
                a(
                    ui,
                    "Middle",
                    canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Middle),
                );
                a(
                    ui,
                    "Bottom",
                    canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Bottom),
                );
                ui.separator();
                let centered_h =
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center);
                let centered =
                    canvas_core::align_vertical(&centered_h, page_h, canvas_core::VAlign::Middle);
                a(ui, "Center on page", centered);
            });
        });
        ui.separator();
        // Estos tres, a diferencia de los de arriba, se resuelven AQUÍ
        // MISMO con `state` directamente — no tocan disco ni el resto de
        // `App`, así que no hace falta pasarlos por `CanvasAction`/`main.rs`.
        let bg_active = state.background_active();
        let bg_can_toggle = bg_active || state.background_source().is_some();
        let mut bg_on = bg_active;
        if ui
            .add_enabled(
                bg_can_toggle,
                egui::Checkbox::new(&mut bg_on, "Blurred background"),
            )
            .clicked()
        {
            state.set_blurred_background(bg_on);
            ui.close();
        }
        let crop_eligible = state
            .selection
            .primary()
            .and_then(|id| state.doc.layer(id).ok())
            .is_some_and(|l| matches!(l.content, LayerContent::Image(_)));
        let mut crop_on = state.crop_mode;
        if ui
            .add_enabled(crop_eligible, egui::Checkbox::new(&mut crop_on, "Crop"))
            .clicked()
        {
            state.crop_mode = crop_on;
            ui.close();
        }
        if ui.button("Size").clicked() {
            state.size_popup = state.doc.page().ok().map(|p| (p.width, p.height));
            ui.close();
        }
    });

    if action.is_none() {
        action = replace_url_popup_ui(state, ui.ctx());
    }

    if rect.width() < 1.0 || rect.height() < 1.0 {
        return action;
    }

    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (1.0, 1.0),
    };

    // La ranura activa siempre conoce su tamaño real (es el documento que se
    // está editando; no hace falta esperar a `DeckProbed`) — mantenerla al
    // día aquí cubre tanto la primera carga como un cambio de tamaño de
    // página desde el panel, sin ningún caso especial.
    let mut sizes_changed = false;
    if let Some(slot) = deck.slots.get_mut(deck.active) {
        if slot.page != Some(page_dims) {
            slot.page = Some(page_dims);
            sizes_changed = true;
        }
    }
    // Defensa en profundidad: cualquier ranura YA cargada (no solo la
    // activa) conoce su tamaño real por su propio documento, sin depender
    // de que `DeckProbed` haya llegado ni de que el sondeo funcione para su
    // formato. Sin esto, un lienzo mayor que la estimación de `Slot::size()`
    // se pinta fuera de su `rect` de layout y se come el hueco con el
    // vecino. Acumulador local porque no se puede escribir
    // `deck.layout_dirty` mientras `&mut deck.slots` sigue prestado por el
    // bucle.
    for slot in &mut deck.slots {
        if let SlotContent::Ready(d) = &slot.content {
            if let Ok(p) = d.doc.page() {
                let real = (p.width, p.height);
                if slot.page != Some(real) {
                    slot.page = Some(real);
                    sizes_changed = true;
                }
            }
        }
    }
    deck.layout_dirty |= sizes_changed;
    if deck.layout_dirty {
        // Recolocar puede desplazar el origen del lienzo ACTIVO como efecto
        // secundario (el centrado en el eje transversal usa el máximo
        // ancho/alto de TODAS las ranuras: aprender el tamaño real de un
        // vecino cambia ese máximo). Compensar el pan para que el lienzo
        // activo se quede clavado en pantalla — el usuario no pidió mover
        // nada. No aplica en el primer frame: `needs_fit`/`AutoFit` van a
        // reescribir el pan entero de todos modos, y no-op con una sola
        // ranura (su origen activo siempre es `(0,0)`).
        let before = deck.active_origin();
        deck.relayout();
        if !state.viewport.needs_fit {
            let after = deck.active_origin();
            let (dx, dy) = (after.0 - before.0, after.1 - before.1);
            if dx != 0.0 || dy != 0.0 {
                state.viewport.pan -= egui::vec2(
                    (dx * state.viewport.zoom) as f32,
                    (dy * state.viewport.zoom) as f32,
                );
            }
        }
    }

    // Salto pedido por la tira, el teclado, un clic directo sobre otro
    // lienzo o "Añadir lienzo": centra sobre el nuevo lienzo activo sin
    // tocar el zoom. También arma `AutoFit::Active` — sin esto, si el modo
    // seguía en `All` (el usuario acababa de pulsar `Ctrl+Alt+0`), el
    // primer redimensionado después de este centrado volvía a encajar TODA
    // la baraja (`resized` más abajo) y deshacía el centrado, con el efecto
    // de "vuelve a la vista de siempre" — el nuevo centrado puntual pasa a
    // ser la referencia, no una excepción que el próximo resize revierta.
    if let Some(target) = state.viewport.center_request.take() {
        state.viewport.center_on(target, rect.size());
        state.viewport.auto_fit = AutoFit::Active;
    }

    // Ajustar el lienzo activo: Ctrl/Cmd+0 o primer frame. Ajustar TODA la
    // baraja: Ctrl+Alt+0 (más específico primero, mismo patrón que
    // redo/undo en `handle_shortcuts`).
    let fit_all = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::ALT,
            egui::Key::Num0,
        ))
    });
    let fit_active = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::Num0,
        ))
    });
    // Reajuste automático si el área de dibujo cambió de tamaño desde el
    // frame anterior (ventana maximizada/restaurada, panel lateral
    // arrastrado): repite el último ajuste automático (`Ctrl+0`/
    // `Ctrl+Alt+0`) mientras siga armado — se desarma en cuanto el usuario
    // hace zoom o paneo a mano (`Viewport::manual_view_change`). SIEMPRE se
    // sella el tamaño, no solo cuando cambia, para no quedar desincronizado.
    let resized = state.viewport.note_size(rect.size());
    if fit_all {
        state.viewport.fit(deck.bounds(), rect.size(), AutoFit::All);
    } else if fit_active || state.viewport.needs_fit {
        state
            .viewport
            .fit(deck.active_rect(), rect.size(), AutoFit::Active);
    } else if resized {
        match state.viewport.auto_fit {
            AutoFit::Active => {
                state
                    .viewport
                    .fit(deck.active_rect(), rect.size(), AutoFit::Active);
            }
            AutoFit::All => {
                state.viewport.fit(deck.bounds(), rect.size(), AutoFit::All);
            }
            AutoFit::Off => {}
        }
    }

    // Zoom pedido desde el menú, anclado al centro del lienzo.
    if let Some(factor) = state.pending_zoom_factor.take() {
        state.viewport.zoom_at(rect.size() / 2.0, factor);
    }

    // Rueda: desplaza a lo largo del eje PRIMARIO de la baraja (Shift = eje
    // transversal); Ctrl+rueda y el pellizco hacen zoom, anclados al cursor.
    // Es uniforme también con un solo lienzo — una excepción "con un archivo
    // hace zoom" se sentiría como un fallo, no como una regla.
    if response.hovered() {
        let (raw_scroll, pinch, pointer, ctrl, shift) = ui.ctx().input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
                i.modifiers.command,
                i.modifiers.shift,
            )
        });
        if ctrl && raw_scroll.y != 0.0 {
            let factor = (f64::from(raw_scroll.y) * 0.0025).exp();
            let anchor = pointer.map_or(rect.size() / 2.0, |p| p - rect.min);
            state.viewport.zoom_at(anchor, factor);
        } else if raw_scroll != egui::Vec2::ZERO {
            // El ratón manda un solo eje de rueda (`raw_scroll.y`); a qué
            // componente del pan va depende de cuál sea el eje primario de
            // la baraja — Shift pide el transversal. Un trackpad que ya
            // manda X (pellizco de dos dedos, Shift+rueda que el propio SO
            // convierte) se respeta tal cual, sin remapear.
            let is_horizontal = matches!(deck.axis, DeckAxis::Horizontal);
            let swap = shift != is_horizontal;
            let delta = if swap && raw_scroll.x == 0.0 {
                egui::vec2(raw_scroll.y, 0.0)
            } else {
                raw_scroll
            };
            // `+=`, no `-=`: mismo signo que el paneo por arrastre de más
            // abajo (`pan += drag_delta`) y que `egui::ScrollArea`
            // (`offset -= scroll_delta`, con `offset` en el sentido
            // contrario a `pan` — es la posición del contenido en pantalla,
            // no cuánto se ha desplazado dentro de él). Con `-=` la rueda
            // quedaba invertida respecto al resto de la propia app.
            state.viewport.manual_view_change();
            state.viewport.pan += delta;
        }
        if (pinch - 1.0).abs() > 1e-4 {
            let anchor = pointer.map_or(rect.size() / 2.0, |p| p - rect.min);
            state.viewport.zoom_at(anchor, f64::from(pinch));
        }
    }

    // Paneo: botón central, o espacio + arrastre primario.
    let space_down = ui.ctx().input(|i| i.key_down(egui::Key::Space));
    let panning = response.dragged_by(egui::PointerButton::Middle)
        || (space_down && response.dragged_by(egui::PointerButton::Primary));
    if panning {
        state.viewport.manual_view_change();
        state.viewport.pan += response.drag_delta();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if space_down && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // Origen del lienzo activo en el espacio de baraja, en puntos de
    // pantalla. `slot_rect` es `rect` desplazado ese origen — pasado en vez
    // de `rect` a `layer_interaction` y a los cuatro ayudantes de
    // coordenadas (`page_to_screen` y compañía, más abajo), consigue que un
    // lienzo que no esté en el origen de la baraja se seleccione/arrastre/
    // redimensione igual que si fuera el único, sin tocar ni una línea de
    // esa lógica.
    let (origin_x, origin_y) = deck.active_origin();
    let slot_offset = egui::vec2(
        (origin_x * state.viewport.zoom) as f32,
        (origin_y * state.viewport.zoom) as f32,
    );
    let slot_rect = egui::Rect::from_min_size(rect.min + slot_offset, rect.size());

    // Qué ranuras están a la vista (con margen de precarga), y sella cuándo
    // se vieron por última vez (política de descarte LRU). Calculado AQUÍ
    // (antes de resolver la pulsación) porque el hit-test de la cabecera de
    // cada lienzo, más abajo, solo tiene sentido sobre ranuras visibles —
    // el resto del uso de `visible` (carga perezosa, descarte, escena,
    // `draw_slot_chrome`) sigue más adelante sin cambios, solo reutiliza
    // este mismo cálculo en vez de repetirlo.
    let (x0, y0) = screen_to_page(&state.viewport, rect, rect.min);
    let (x1, y1) = screen_to_page(&state.viewport, rect, rect.max);
    let view_deck_rect = Deck::dilate(DeckRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    });
    let visible = deck.visible_indices(view_deck_rect);
    deck.mark_visible(&visible);

    // Pulsación sobre un lienzo que no es el activo: lo activa (el
    // intercambio en sí lo aplica `deck::apply_jump`, fuera de este módulo,
    // para no mutar el documento activo a mitad de este mismo frame). Se
    // decide en el frame de la PULSACIÓN, no en el de soltar limpio
    // (`response.clicked()`, que solo se cumple si el puntero no se movió
    // más allá del umbral de arrastre de egui entre pulsar y soltar — un
    // clic humano real casi nunca es tan quieto). Cuando egui clasifica esa
    // pulsación como arrastre en vez de clic, `clicked()` no llega nunca, y
    // `layer_interaction` SÍ corría: como usa `slot_rect` (siempre el
    // espacio de la ranura ACTIVA), un clic con el más mínimo temblor sobre
    // OTRO lienzo agarraba y movía una capa del documento activo — la
    // «Position X» que cambiaba sola en el panel de propiedades sin que el
    // usuario tocase ninguna capa.
    if ui.input(|i| i.pointer.primary_pressed()) && !space_down && response.contains_pointer() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            // Cabecera de CUALQUIER lienzo visible (activo o no) se
            // comprueba PRIMERO, en espacio de pantalla — antes del hit-test
            // en espacio de página de más abajo, que solo conoce el cuerpo
            // del lienzo, no su cabecera (que vive por encima, fuera de su
            // `DeckRect`). Un acierto aquí consume la pulsación entera: no
            // cae al hit-test de activación ni a `layer_interaction`.
            let mut header_hit = false;
            for &idx in &visible {
                if header_hit {
                    break;
                }
                let Some((id, is_placeholder, s_rect)) = deck
                    .slots
                    .get(idx)
                    .map(|s| (s.id, s.is_placeholder, s.rect))
                else {
                    continue;
                };
                let top_left = page_to_screen(&state.viewport, rect, s_rect.x, s_rect.y);
                let top_right =
                    page_to_screen(&state.viewport, rect, s_rect.x + s_rect.w, s_rect.y);
                let Some(header) = slot_header_layout(top_left.x, top_right.x, top_left.y) else {
                    continue;
                };
                if !is_placeholder && header.name.contains(pos) {
                    // Ver `draw_rename_overlay`: el propio cuadro de texto
                    // se dibuja ahí, en un `egui::Area` de primer plano, no
                    // aquí — aquí solo se arma el estado y se le pide el
                    // foco.
                    let stem = deck
                        .slots
                        .get(idx)
                        .and_then(|s| s.path.file_stem())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    deck.rename_edit = Some((id, stem));
                    ui.memory_mut(|m| {
                        m.request_focus(egui::Id::new(("canvas_slot_rename", id)));
                    });
                    header_hit = true;
                } else if header.prev.contains(pos) {
                    deck.move_slot(id, MoveDir::Prev);
                    header_hit = true;
                } else if header.next.contains(pos) {
                    deck.move_slot(id, MoveDir::Next);
                    header_hit = true;
                } else if header.lock.contains(pos) {
                    if let Some(s) = deck.slots.get_mut(idx) {
                        s.locked = !s.locked;
                    }
                    header_hit = true;
                } else if header.dup.contains(pos) {
                    action = Some(CanvasAction::Duplicate(id));
                    header_hit = true;
                } else if header.del.contains(pos) {
                    action = Some(CanvasAction::Delete(id));
                    header_hit = true;
                }
            }
            if header_hit {
                state.press_on_other_slot = true;
            } else {
                let (dx, dy) = screen_to_page(&state.viewport, rect, pos);
                let target = deck.slots.iter().position(|s| {
                    dx >= s.rect.x
                        && dx <= s.rect.x + s.rect.w
                        && dy >= s.rect.y
                        && dy <= s.rect.y + s.rect.h
                });
                if let Some(idx) = target {
                    if idx != deck.active {
                        deck.jump_to = Some(idx);
                        // A petición del usuario: cambiar de activo desde el
                        // área central SIEMPRE centra la vista sobre él, en
                        // vez de dejarlo donde cayera (antes solo la tira o
                        // el teclado pedían recentrado) — así no hace falta
                        // ir a buscarlo tras el salto.
                        deck.jump_center = true;
                        state.press_on_other_slot = true;
                    }
                } else if deck.folder.is_some()
                    && dx >= deck.add_zone.x
                    && dx <= deck.add_zone.x + deck.add_zone.w
                    && dy >= deck.add_zone.y
                    && dy <= deck.add_zone.y + deck.add_zone.h
                {
                    // Pulsación sobre la zona "+" al final de la baraja: crea
                    // y activa un lienzo en blanco, igual que
                    // `App::add_canvas` (botón de la tira) pero resuelto
                    // aquí mismo — es una operación puramente en memoria, no
                    // toca disco ni el watcher, así que no hace falta pasar
                    // por `main.rs`.
                    if let Some(idx) =
                        deck.push_placeholder((deck.add_zone.w, deck.add_zone.h), new_canvas_ext)
                    {
                        deck.jump_to = Some(idx);
                        deck.jump_center = true;
                        state.press_on_other_slot = true;
                    }
                }
            }
        }
    }

    // Selección, arrastre y redimensionado (si no se está paneando, el
    // gesto en curso no pertenece a la baraja, y el diseño activo no está
    // bloqueado — `Slot::locked`, cabecera del lienzo).
    let active_locked = deck.slots.get(deck.active).is_some_and(|s| s.locked);
    if !panning && !space_down && !state.press_on_other_slot && !active_locked {
        layer_interaction(state, ui, &response, slot_rect);
    }

    // La marca dura el gesto entero (pulsar, arrastrar, soltar) y se limpia
    // cuando el botón ya no está pulsado — DESPUÉS de la guardia de arriba,
    // para que el frame en que se suelta (donde egui emite `clicked`/
    // `drag_stopped`) siga protegido.
    if !ui.input(|i| i.pointer.primary_down()) {
        state.press_on_other_slot = false;
    }

    // Render vello → textura del tamaño físico del lienzo.
    let ppp = ui.ctx().pixels_per_point();
    let width = (rect.width() * ppp).round().max(1.0) as u32;
    let height = (rect.height() * ppp).round().max(1.0) as u32;
    let surface = CanvasSurface::ensure(surface_slot, rs, width, height);

    // Transformación del espacio de BARAJA a píxeles físicos del lienzo, sin
    // desplazar por ninguna ranura en particular: cada lienzo visible añade
    // su propio origen antes de renderizarse (más abajo).
    let base_view = Affine::translate((
        f64::from(state.viewport.pan.x * ppp),
        f64::from(state.viewport.pan.y * ppp),
    )) * Affine::scale(state.viewport.zoom * f64::from(ppp));

    // Carga perezosa: pide las ranuras `Idle` visibles (o del radio de
    // precarga alrededor de la activa) que quepan en el presupuesto de
    // cargas en vuelo.
    if let Some(folder) = deck.folder.clone() {
        for path in deck.request_loads(&visible) {
            loader::spawn_load_slot(folder.clone(), path, tx.clone(), ui.ctx().clone());
        }
    }
    // Descarte: libera memoria de ranuras lejanas, limpias y sin guardado en
    // curso, por encima del presupuesto.
    for scope in deck.evict() {
        renderer.forget_scope(scope);
    }

    // Una sola escena para todos los lienzos visibles ya cargados (activo o
    // `Ready`); el resto se pinta encima como miniatura/placeholder, con el
    // `Painter` normal de egui (más abajo, en `draw_slot_chrome`) — ya están
    // en GPU desde la galería, no hace falta subirlas de nuevo a vello.
    let mut scene = vello::Scene::new();
    for &idx in &visible {
        let Some(slot) = deck.slots.get(idx) else {
            continue;
        };
        let scope = FxScope(slot.id);
        let view = base_view * Affine::translate(slot.rect.origin());
        if idx == deck.active {
            sync_and_append(
                &mut scene,
                renderer,
                rs,
                &state.doc,
                &state.images,
                scope,
                view,
            );
        } else if let SlotContent::Ready(doc) = &slot.content {
            sync_and_append(&mut scene, renderer, rs, &doc.doc, &doc.images, scope, view);
        }
    }
    if let Err(e) = surface.render(rs, renderer, &scene) {
        tracing::error!("fallo renderizando el lienzo: {e}");
    }

    ui.painter().image(
        surface.egui_id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    for &idx in &visible {
        draw_slot_chrome(state, deck, idx, ui, rect);
    }
    draw_add_zone(state, deck, ui, rect);
    draw_header_tooltips(deck, ui, rect, &state.viewport, &visible);

    if state.show_grid {
        draw_grid(state, ui, slot_rect, rect, page_dims);
    }
    draw_selection_overlay(state, ui, slot_rect, rect);
    if state.show_rulers {
        draw_rulers(state, ui, slot_rect, rect);
    }

    // Renombrado en curso desde una cabecera (si lo hay): `egui::Area` de
    // primer plano, PASO DE UI SEPARADO del `response` gigante de arriba —
    // ver la doc de `draw_rename_overlay`.
    if let Some(a) = draw_rename_overlay(state, deck, ui, rect) {
        action = Some(a);
    }
    size_popup_ui(state, ui.ctx());
    action
}

/// Sincroniza los efectos GPU de un documento y lo añade a `scene` con su
/// propia transformación de vista. Un `fn` normal, no un cierre: dentro del
/// bucle de `canvas_ui` se llama con `renderer`/`scene` prestados de forma
/// disjunta en cada iteración, y un cierre que capturase ambos a la vez
/// complicaría el préstamo sin necesidad.
#[allow(clippy::too_many_arguments)]
fn sync_and_append(
    scene: &mut vello::Scene,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    doc: &Document,
    images: &ImageMap,
    scope: FxScope,
    view: Affine,
) {
    if let Ok(page) = doc.page() {
        let fx_targets: Vec<(LayerId, canvas_core::Effects)> =
            page.layers.iter().map(|l| (l.id, l.effects)).collect();
        for (id, effects) in fx_targets {
            if let Some(source) = images.get(&id) {
                renderer.sync_layer_effects(&rs.device, &rs.queue, scope, id, source, &effects);
            }
        }
    }
    let blurred = renderer.blur_overrides(scope);
    canvas_render::append_document(scene, doc, images, &blurred, view, true);
}
