//! Traduce un clic de menú (barra nativa o de respaldo) a la acción de
//! `AppInner` correspondiente, sobre el workspace que mostró el menú.

use eframe::egui;

use crate::{clipboard, editor, export, layers_panel, loader, menus};

use super::{AppInner, Nav, View, Workspace};

impl AppInner {
    /// Traduce un clic de menú a la acción correspondiente sobre `ws`.
    pub(super) fn handle_menu_action(
        &mut self,
        ws: &mut Workspace,
        action: menus::MenuAction,
        ctx: &egui::Context,
    ) {
        use menus::MenuAction as A;
        match action {
            // Ventana nueva (workspace) con la bienvenida, desde cualquier
            // ventana — el conmutador (Ctrl+Tab) salta entre ellas.
            A::NewWindow => {
                self.new_workspace();
                self.pending_focus = Some(self.workspaces.len() - 1);
                self.request_repaint_all_viewports(ctx);
            }
            A::NewDesign => {
                let gallery_seed = if let View::Gallery(g) = &mut ws.view {
                    Some(crate::deck::DeckSeed::from_gallery(g))
                } else {
                    None
                };
                if let Some(seed) = gallery_seed {
                    self.request_nav(ws, Nav::NewDesignInFolder { seed }, ctx);
                } else {
                    self.request_nav(ws, Nav::NewDesign, ctx);
                }
            }
            A::OpenFile => loader::spawn_pick_file(ws.tx.clone(), ctx.clone()),
            A::OpenFolder => loader::spawn_pick_folder(ws.tx.clone(), ctx.clone()),
            A::OpenRecent(path) => self.request_nav(ws, Nav::Open(path), ctx),
            A::CloseProject => self.request_nav(ws, Nav::CloseProject, ctx),
            A::Save => {
                if let View::Editor(state) = &mut ws.view {
                    state.save_clicked = true;
                }
            }
            A::SaveAs => {
                if let View::Editor(state) = &mut ws.view {
                    state.save_as_clicked = true;
                }
            }
            A::SaveAll => self.start_save_all(ws),
            A::Export => {
                if let View::Editor(_) = &ws.view {
                    ws.export.export_dialog = Some(export::ExportDialog::default());
                }
            }
            A::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            A::Undo => {
                if let View::Editor(state) = &mut ws.view {
                    state.undo();
                }
            }
            A::Redo => {
                if let View::Editor(state) = &mut ws.view {
                    state.redo();
                }
            }
            A::ZoomIn => {
                if let View::Editor(state) = &mut ws.view {
                    state.pending_zoom_factor = Some(1.25);
                }
            }
            A::ZoomOut => {
                if let View::Editor(state) = &mut ws.view {
                    state.pending_zoom_factor = Some(0.8);
                }
            }
            A::FitToWindow => {
                if let View::Editor(state) = &mut ws.view {
                    state.viewport.request_fit();
                }
            }
            A::ToggleGrid => {
                if let View::Editor(state) = &mut ws.view {
                    state.show_grid = !state.show_grid;
                }
            }
            A::ToggleRulers => {
                if let View::Editor(state) = &mut ws.view {
                    state.show_rulers = !state.show_rulers;
                }
            }
            A::NextCanvas => {
                if let View::Editor(state) = &mut ws.view {
                    state.deck_nav = Some(editor::DeckNav::Next);
                }
            }
            A::PrevCanvas => {
                if let View::Editor(state) = &mut ws.view {
                    state.deck_nav = Some(editor::DeckNav::Prev);
                }
            }
            A::ToggleCanvasesPanel => {
                ws.deck.strip_visible = !ws.deck.strip_visible;
                self.settings.deck_strip_visible = ws.deck.strip_visible;
                self.settings.save_in_background();
            }
            A::ToggleCanvasesAxis => self.toggle_deck_axis(ws),
            A::CycleCanvasesSide => self.cycle_strip_side(ws),
            A::ToggleLayersPanel => {
                self.settings.layers_collapsed = !self.settings.layers_collapsed;
                self.settings.save_in_background();
            }
            A::AddCanvas => self.add_canvas(ws),
            A::FullScreen => {
                let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            A::Settings => ws.show_settings = true,
            A::About => ws.show_about = true,
            A::Cut => {
                if let View::Editor(state) = &mut ws.view {
                    clipboard::cut(state);
                }
            }
            A::Copy => {
                if let View::Editor(state) = &ws.view {
                    clipboard::copy(state);
                }
            }
            A::Paste => {
                if let View::Editor(state) = &mut ws.view {
                    if !clipboard::paste(state) {
                        state.save_error = Some(clipboard::PASTE_EMPTY_MSG.to_owned());
                    }
                }
            }
            A::Duplicate => {
                if let View::Editor(state) = &mut ws.view {
                    clipboard::duplicate(state);
                }
            }
            A::Delete => {
                if let View::Editor(state) = &mut ws.view {
                    editor::delete_selected(state);
                }
            }
            A::SelectAll => {
                if let View::Editor(state) = &mut ws.view {
                    clipboard::select_all(state);
                }
            }
            A::Group => {
                if let View::Editor(state) = &mut ws.view {
                    layers_panel::group_selection(state);
                }
            }
            A::Ungroup => {
                if let View::Editor(state) = &mut ws.view {
                    layers_panel::ungroup_selection(state);
                }
            }
        }
    }
}
