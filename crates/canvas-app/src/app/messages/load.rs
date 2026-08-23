//! Respuestas del hilo de disco que traen PIXELES: la imagen abierta en el
//! editor, una imagen que se anade o sustituye una capa, un lienzo de fondo de
//! la baraja, y el sondeo de tamanos de pagina de una carpeta.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, editor, loader};

use super::super::{App, View};

impl App {
    pub(super) fn on_image_loaded(
        &mut self,
        path: PathBuf,
        result: Result<loader::LoadOutcome, String>,
        metadata: canvas_io::ImageMetadata,
        ctx: &egui::Context,
    ) {
        // Ignora cargas que ya no corresponden a la vista actual.
        let expected = matches!(&self.view, View::Loading { path: p } if *p == path);
        if !expected {
            return;
        }
        let metadata = (!metadata.is_empty()).then_some(metadata);
        match result {
            Ok(loader::LoadOutcome::Restored(restored)) => {
                // Si la imagen cambió por fuera desde el último
                // guardado con capas, avisa y deja elegir.
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
                    self.resolve_deck(&path, ctx);
                    let mut state = editor::EditorState::from_restored(path.clone(), restored);
                    state.from_gallery = self.deck.folder.clone();
                    state.sidecar_enabled = self.settings.sidecar_default;
                    state.source_metadata = metadata;
                    self.remember_page_size(&state.doc);
                    self.view = View::Editor(Box::new(state));
                } else {
                    // Recarga plana, ignorando el sidecar.
                    loader::spawn_load_image(path.clone(), false, self.tx.clone(), ctx.clone());
                    self.view = View::Loading { path: path.clone() };
                }
            }
            Ok(loader::LoadOutcome::Design(restored)) => {
                // Diseño autónomo: `hash_matches` siempre es
                // `true` (no hay nada que contrastar), así que no
                // hace falta el diálogo de «cambió por fuera».
                self.resolve_deck(&path, ctx);
                let mut state = editor::EditorState::from_design(path.clone(), restored);
                state.from_gallery = self.deck.folder.clone();
                self.remember_page_size(&state.doc);
                self.view = View::Editor(Box::new(state));
            }
            Ok(loader::LoadOutcome::Flat(img)) => {
                match editor::EditorState::from_image(path.clone(), img) {
                    Ok(mut state) => {
                        self.resolve_deck(&path, ctx);
                        state.from_gallery = self.deck.folder.clone();
                        state.sidecar_enabled = self.settings.sidecar_default;
                        state.source_metadata = metadata;
                        self.remember_page_size(&state.doc);
                        self.view = View::Editor(Box::new(state));
                    }
                    Err(e) => {
                        self.view = View::Welcome {
                            error: Some(format!("Could not open \"{}\": {e}", path.display())),
                        };
                    }
                }
            }
            Err(e) => {
                self.view = View::Welcome {
                    error: Some(format!("Could not open \"{}\": {e}", path.display())),
                };
            }
        }
        self.sync_title(ctx);
    }

    pub(super) fn on_image_loaded_for_layer(
        &mut self,
        path: PathBuf,
        result: Result<canvas_io::LoadedImage, String>,
    ) {
        if let View::Editor(state) = &mut self.view {
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
        layer: canvas_core::LayerId,
        label: String,
        source_path: Option<PathBuf>,
        result: Result<canvas_io::LoadedImage, String>,
    ) {
        if let View::Editor(state) = &mut self.view {
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
        folder: PathBuf,
        generation: u64,
        path: PathBuf,
        result: Result<deck::SlotDoc, String>,
    ) {
        if self.deck.accepts_response(&folder, generation) {
            self.deck.loading_finished();
            if let Some(idx) = self.deck.find_by_path(&path) {
                let still_loading = self
                    .deck
                    .slots
                    .get(idx)
                    .is_some_and(|slot| matches!(slot.content, deck::SlotContent::Loading));
                if still_loading {
                    let content = result.map_or_else(deck::SlotContent::Failed, |doc| {
                        deck::SlotContent::Ready(Box::new(doc))
                    });
                    if let Some(slot) = self.deck.slots.get_mut(idx) {
                        slot.content = content;
                    }
                }
            }
        }
    }

    pub(super) fn on_deck_probed(
        &mut self,
        folder: PathBuf,
        generation: u64,
        sizes: Vec<(PathBuf, Option<(f64, f64)>)>,
    ) {
        if self.deck.accepts_response(&folder, generation) {
            self.deck.set_probes(sizes);
        }
    }
}
