//! Traduce un clic de menú (barra nativa o de respaldo) a la acción de
//! `App` correspondiente.

use eframe::egui;

use crate::{clipboard, editor, export, layers_panel, loader, menus};

use super::{App, Nav, View};

impl App {
    /// Traduce un clic de menú a la acción correspondiente.
    pub(super) fn handle_menu_action(&mut self, action: menus::MenuAction, ctx: &egui::Context) {
        use menus::MenuAction as A;
        match action {
            // Desde una galería, Ctrl+N crea el diseño DENTRO de esa carpeta
            // en vez de un documento fantasma sin sitio en disco.
            A::NewDesign => {
                if let View::Gallery(g) = &self.view {
                    loader::spawn_gallery_op(
                        loader::GalleryOp::NewDesign {
                            folder: g.folder.clone(),
                            page: self.settings.last_page_size,
                            ext: self.settings.new_canvas_format.extension().to_owned(),
                            jpeg_quality: self.settings.jpeg_quality,
                        },
                        true,
                        self.tx.clone(),
                        ctx.clone(),
                    );
                } else {
                    self.request_nav(Nav::NewDesign, ctx);
                }
            }
            A::OpenFile => loader::spawn_pick_file(self.tx.clone(), ctx.clone()),
            A::OpenFolder => loader::spawn_pick_folder(self.tx.clone(), ctx.clone()),
            A::OpenRecent(path) => self.request_nav(Nav::Open(path), ctx),
            A::CloseProject => self.request_nav(Nav::CloseProject, ctx),
            A::Save => {
                if let View::Editor(state) = &mut self.view {
                    state.save_clicked = true;
                }
            }
            A::SaveAs => {
                if let View::Editor(state) = &mut self.view {
                    state.save_as_clicked = true;
                }
            }
            A::SaveAll => self.start_save_all(),
            A::Export => {
                if let View::Editor(_) = &self.view {
                    self.export_dialog = Some(export::ExportDialog::default());
                }
            }
            A::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            A::Undo => {
                if let View::Editor(state) = &mut self.view {
                    state.undo();
                }
            }
            A::Redo => {
                if let View::Editor(state) = &mut self.view {
                    state.redo();
                }
            }
            A::ZoomIn => {
                if let View::Editor(state) = &mut self.view {
                    state.pending_zoom_factor = Some(1.25);
                }
            }
            A::ZoomOut => {
                if let View::Editor(state) = &mut self.view {
                    state.pending_zoom_factor = Some(0.8);
                }
            }
            A::FitToWindow => {
                if let View::Editor(state) = &mut self.view {
                    state.viewport.request_fit();
                }
            }
            A::ToggleGrid => {
                if let View::Editor(state) = &mut self.view {
                    state.show_grid = !state.show_grid;
                }
            }
            A::ToggleRulers => {
                if let View::Editor(state) = &mut self.view {
                    state.show_rulers = !state.show_rulers;
                }
            }
            A::NextCanvas => {
                if let View::Editor(state) = &mut self.view {
                    state.deck_nav = Some(editor::DeckNav::Next);
                }
            }
            A::PrevCanvas => {
                if let View::Editor(state) = &mut self.view {
                    state.deck_nav = Some(editor::DeckNav::Prev);
                }
            }
            A::ToggleCanvasesPanel => {
                self.deck.strip_visible = !self.deck.strip_visible;
                self.settings.deck_strip_visible = self.deck.strip_visible;
                self.settings.save_in_background();
            }
            A::ToggleCanvasesAxis => self.toggle_deck_axis(),
            A::CycleCanvasesSide => self.cycle_strip_side(),
            A::AddCanvas => self.add_canvas(),
            A::FullScreen => {
                let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            A::Settings => self.show_settings = true,
            A::About => self.show_about = true,
            A::Cut => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::cut(state);
                }
            }
            A::Copy => {
                if let View::Editor(state) = &self.view {
                    clipboard::copy(state);
                }
            }
            A::Paste => {
                if let View::Editor(state) = &mut self.view {
                    if !clipboard::paste(state) {
                        state.save_error = Some(clipboard::PASTE_EMPTY_MSG.to_owned());
                    }
                }
            }
            A::Duplicate => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::duplicate(state);
                }
            }
            A::Delete => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::delete_selected(state);
                }
            }
            A::SelectAll => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::select_all(state);
                }
            }
            A::Group => {
                if let View::Editor(state) = &mut self.view {
                    layers_panel::group_selection(state);
                }
            }
            A::Ungroup => {
                if let View::Editor(state) = &mut self.view {
                    layers_panel::ungroup_selection(state);
                }
            }
        }
    }
}
