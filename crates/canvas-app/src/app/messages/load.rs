//! Respuestas del hilo de disco que traen PIXELES: la imagen abierta en el
//! editor, una imagen que se anade o sustituye una capa, un lienzo de fondo de
//! la baraja, y el sondeo de tamanos de pagina de una carpeta.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, editor, loader};

use super::super::{AppInner, View, Workspace};

impl AppInner {
    pub(super) fn on_image_loaded(
        &mut self,
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<loader::LoadOutcome, String>,
        metadata: canvas_io::ImageMetadata,
        ctx: &egui::Context,
    ) {
        // Ignora cargas que ya no corresponden a la vista actual de ESTA ventana.
        let expected = matches!(&ws.view, View::Loading { path: p } if *p == path);
        if !expected {
            return;
        }
        let metadata = (!metadata.is_empty()).then_some(metadata);
        match result {
            Ok(loader::LoadOutcome::Restored(restored)) => {
                let use_layers = restored.hash_matches
                    || {
                        let choice = rfd::MessageDialog::new()
                            .set_level(rfd::MessageLevel::Warning)
                            .set_title("Image changed outside Canvas Desktop")
                            .set_description(format!(
                                "\"{}\" was modified by another program after the last save with layers.\nRestore the editable layers anyway? (\"No\" opens the image as it is now.)",
                                path.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                            ))
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show();
                        matches!(choice, rfd::MessageDialogResult::Yes)
                    };
                if use_layers {
                    self.resolve_deck(ws, &path, ctx);
                    let mut state = editor::EditorState::from_restored(path.clone(), restored);
                    state.from_gallery = ws.deck.folder.clone();
                    state.sidecar_enabled = self.settings.sidecar_default;
                    state.source_metadata = metadata;
                    self.remember_page_size(ws, &state.doc);
                    ws.view = View::Editor(Box::new(state));
                } else {
                    loader::spawn_load_image(path.clone(), false, ws.tx.clone(), ctx.clone());
                    ws.view = View::Loading { path: path.clone() };
                }
            }
            Ok(loader::LoadOutcome::Design(restored)) => {
                self.resolve_deck(ws, &path, ctx);
                let mut state = editor::EditorState::from_design(path.clone(), restored);
                state.from_gallery = ws.deck.folder.clone();
                self.remember_page_size(ws, &state.doc);
                ws.view = View::Editor(Box::new(state));
            }
            Ok(loader::LoadOutcome::Flat(img)) => {
                match editor::EditorState::from_image(path.clone(), img) {
                    Ok(mut state) => {
                        self.resolve_deck(ws, &path, ctx);
                        state.from_gallery = ws.deck.folder.clone();
                        state.sidecar_enabled = self.settings.sidecar_default;
                        state.source_metadata = metadata;
                        self.remember_page_size(ws, &state.doc);
                        ws.view = View::Editor(Box::new(state));
                    }
                    Err(e) => {
                        ws.view = View::Welcome {
                            error: Some(format!("Could not open \"{}\": {e}", path.display())),
                        };
                    }
                }
            }
            Err(e) => {
                ws.view = View::Welcome {
                    error: Some(format!("Could not open \"{}\": {e}", path.display())),
                };
            }
        }
        self.sync_title(ctx, ws);
    }

    pub(super) fn on_image_loaded_for_layer(
        &mut self,
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<canvas_io::LoadedImage, String>,
    ) {
        if let View::Editor(state) = &mut ws.view {
            match result {
                Ok(img) => {
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Image".to_owned());
                    state.add_image_layer(name, Some(path), img);
                }
                Err(e) => {
                    state.save_error = Some(format!("Could not add \"{}\": {e}", path.display()));
                }
            }
        }
    }

    pub(super) fn on_image_loaded_for_replace(
        &mut self,
        ws: &mut Workspace,
        layer: canvas_core::LayerId,
        label: String,
        source_path: Option<PathBuf>,
        result: Result<canvas_io::LoadedImage, String>,
    ) {
        if let View::Editor(state) = &mut ws.view {
            match result {
                Ok(img) => {
                    if let Err(e) = state.replace_image_layer(layer, source_path, img) {
                        state.save_error = Some(format!("Could not replace image: {e}"));
                    }
                }
                Err(e) => {
                    state.save_error = Some(format!("Could not replace with {label}: {e}"));
                }
            }
        }
    }

    pub(super) fn on_slot_prepared(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        generation: u64,
        path: PathBuf,
        result: Result<deck::SlotDoc, String>,
    ) {
        if ws.deck.accepts_response(&folder, generation) {
            ws.deck.loading_finished();
            if let Some(idx) = ws.deck.find_by_path(&path) {
                let still_loading = ws
                    .deck
                    .slots
                    .get(idx)
                    .is_some_and(|slot| matches!(slot.content, deck::SlotContent::Loading));
                if still_loading {
                    let content = result.map_or_else(deck::SlotContent::Failed, |doc| {
                        deck::SlotContent::Ready(Box::new(doc))
                    });
                    if let Some(slot) = ws.deck.slots.get_mut(idx) {
                        slot.content = content;
                    }
                }
            }
        }
    }

    pub(super) fn on_deck_probed(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        generation: u64,
        sizes: Vec<(PathBuf, Option<(f64, f64)>)>,
    ) {
        if ws.deck.accepts_response(&folder, generation) {
            ws.deck.set_probes(sizes);
        }
    }
}
