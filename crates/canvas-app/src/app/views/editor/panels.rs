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
    let mut strip_action = None;
    let mut canvas_action = None;
    let properties_max = (ui.available_width() * 0.45).clamp(160.0, 400.0);
    let properties_min = 220.0_f32.min(properties_max);
    if state.framing.is_some() {
        egui::Panel::right("properties")
            .default_size(260.0)
            .size_range(properties_min..=properties_max)
            .resizable(true)
            .show(ui, |ui| editor::properties_ui(state, ui));
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(session) = &mut state.framing {
                session.canvas(ui);
            }
        });
        return (None, None);
    }
    // Barra de estado, PRIMERA de los paneles inferiores: se queda pegada al
    // borde inferior de la ventana y la tira de la baraja (si cae en el
    // borde inferior) queda encima, sin robarle ni un píxel al lienzo.
    egui::Panel::bottom("editor_status_bar")
        .exact_size(20.0)
        .resizable(false)
        .show(ui, |ui| {
            status_bar_ui(f, ui);
        });
    let narrow = ui.available_width() < 1100.0;
    if f.deck.is_visible() && !state.isolate && !narrow {
        let active_dirty = state.is_dirty();
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
    let locked =
        state.ytdlp.edit.is_some() || f.deck.slots.get(f.deck.active).is_some_and(|s| s.locked);
    let layers_collapsed_before = f.settings.layers_collapsed;
    let narrow_id = egui::Id::new(("narrow-tools-collapsed", ui.ctx().viewport_id()));
    let mut layers_collapsed = if narrow {
        ui.data(|d| d.get_temp::<bool>(narrow_id).unwrap_or(true))
    } else {
        f.settings.layers_collapsed
    };
    const COLLAPSED_WIDTH: f32 = 36.0;
    // Todas las pestañas comparten el ancho elegido al arrastrar el borde.
    let expanded_width =
        egui::containers::panel::PanelState::load(ui.ctx(), egui::Id::new("layers"))
            .map_or(220.0, |panel| panel.size().x);
    const PANEL_ANIM_SECS: f64 = 0.2;

    let anim_salt = egui::Id::new("layers_panel_anim");
    type AnimState = (bool, f64, bool);
    let mut anim: Option<AnimState> = ui.data_mut(|d| d.get_temp(anim_salt).unwrap_or(None));
    let now: f64 = ui.input(|i| i.time);
    let target_collapsed = layers_collapsed;

    if anim.is_none_or(|(t, _, _)| t != target_collapsed) {
        anim = Some((target_collapsed, now, false));
    }

    let mut width = if target_collapsed {
        COLLAPSED_WIDTH
    } else {
        expanded_width
    };
    if let Some((target, start, started)) = &mut anim {
        if !*started {
            *start = now;
            *started = true;
        }
        let elapsed = now - *start;
        if elapsed >= PANEL_ANIM_SECS || f.settings.reduced_motion || narrow {
            // Animación terminada: NO ponemos `anim = None` porque en el
            // frame siguiente `None` se interpretaría como «arranca una
            // animación nueva» y el panel rebotaría sin fin.
        } else {
            let t = (elapsed / PANEL_ANIM_SECS) as f32;
            let ease = 1.0 - (1.0 - t).powi(3);
            if *target {
                width = expanded_width + (COLLAPSED_WIDTH - expanded_width) * ease;
            } else {
                width = COLLAPSED_WIDTH + (expanded_width - COLLAPSED_WIDTH) * ease;
            }
            ui.ctx().request_repaint();
        }
    }
    ui.data_mut(|d| d.insert_temp(anim_salt, anim));

    let midpoint = (COLLAPSED_WIDTH + expanded_width) * 0.5;
    let animating = anim.is_some_and(|(_, start, _)| now - start < PANEL_ANIM_SECS)
        && !f.settings.reduced_motion
        && !narrow;
    // La animación y el estado cerrado no sobrescriben el ancho expandido.
    let panel_id = if target_collapsed || animating {
        "layers_transition"
    } else {
        "layers"
    };
    let panel = egui::Panel::left(panel_id).frame(egui::Frame::NONE);
    let panel = if target_collapsed || animating {
        panel.exact_size(width).resizable(false)
    } else {
        panel
            .default_size(expanded_width)
            .size_range(180.0..=420.0_f32.min(ui.available_width() * 0.65).max(180.0))
            .resizable(true)
    };
    panel.show(ui, |ui| {
        crate::sidebar::fixed_width_ui(ui, |ui| {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
            if width < midpoint {
                let new_order = layers_panel::vertical_tab_strip_ui(
                    ui,
                    &mut state.active_left_tab,
                    &mut layers_collapsed,
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
                    // Destino de inserción web/Unsplash (A07): la ranura
                    // activa de ESTA baraja en ESTA generación.
                    let insert_dest = crate::loader::ImageInsertDest {
                        generation: f.deck.generation(),
                        slot_id: f.deck.slots.get(f.deck.active).map_or(u64::MAX, |s| s.id),
                    };
                    let new_order = layers_panel::left_panel_ui(
                        state,
                        ui,
                        &mut *f.settings,
                        f.deck.folder.clone(),
                        insert_dest,
                        f.tx,
                        &mut layers_collapsed,
                    );
                    if let Some(new_order) = new_order {
                        if f.settings.layers_tab_order != new_order {
                            f.settings.layers_tab_order = new_order;
                            f.settings.save_in_background();
                        }
                    }
                });
            }
        })
    });
    if narrow {
        ui.data_mut(|d| d.insert_temp(narrow_id, layers_collapsed));
    } else {
        f.settings.layers_collapsed = layers_collapsed;
    }
    if f.settings.layers_collapsed != layers_collapsed_before {
        f.settings.save_in_background();
    }
    if !narrow || target_collapsed {
        egui::Panel::right("properties")
            .default_size(260.0)
            .size_range(properties_min..=properties_max)
            .resizable(true)
            .show(ui, |ui| {
                ui.add_enabled_ui(!locked, |ui| editor::properties_ui(state, ui));
            });
    }
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
            ui.add_enabled_ui(state.ytdlp.edit.is_none(), |ui| {
                canvas_action = editor::canvas_ui(state, f.deck, ui, &mut ctx);
            });
        });

    (strip_action, canvas_action)
}

/// Barra de estado del editor: RAM libre, presupuesto GPU de efectos y bytes
/// FX en uso. La RAM libre se pinta por umbrales — naranja por debajo de 2
/// GiB (presupuesto reducido) y rojo por debajo de 512 MiB (guardado
/// bloqueado) — para que se vea de un vistazo cuándo el presupuesto se está
/// reduciendo. Pura lectura: no roba eventos y cuesta una syscall de RAM
/// (la misma señal que ya usan la caché y los guards de persistencia).
fn status_bar_ui(f: &EditorFrame<'_>, ui: &mut egui::Ui) {
    let free = deck::free_ram_bytes();
    let budget = canvas_render::resolve_fx_budget(deck::total_physical_ram_bytes(), free);
    let used = f.renderer.fx_total_bytes();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new(crate::i18n::tr("Free RAM")).weak());
        match free {
            Some(bytes) => {
                let (color, note) = if deck::is_critical_free_ram(free) {
                    (
                        ui.visuals().error_fg_color,
                        " · crítica (guardado bloqueado)",
                    )
                } else if bytes < deck::FREE_RAM_REDUCTION_THRESHOLD_BYTES {
                    (
                        egui::Color32::from_rgb(230, 170, 50),
                        " · presupuesto reducido",
                    )
                } else {
                    (ui.visuals().text_color(), "")
                };
                ui.colored_label(color, format!("{}{note}", fmt_bytes(bytes)));
            }
            None => {
                ui.label(egui::RichText::new(crate::i18n::tr("—")).weak());
            }
        }
        ui.separator();
        ui.label(egui::RichText::new(crate::i18n::tr("FX GPU")).weak());
        ui.label(format!("{} / {}", fmt_bytes(used), fmt_bytes(budget)));
    });
}

/// Formatea bytes para la barra de estado: GiB con un decimal, MiB entero,
/// bytes a secas por debajo.
fn fmt_bytes(bytes: u64) -> String {
    const GIB: u64 = 1 << 30;
    const MIB: u64 = 1 << 20;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.0} MiB", bytes as f64 / MIB as f64)
    } else {
        format!("{bytes} B")
    }
}
