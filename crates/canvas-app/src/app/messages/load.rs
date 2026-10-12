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
        result: Result<loader::LoadOutcome, canvas_io::IoError>,
        metadata: canvas_io::ImageMetadata,
        ctx: &egui::Context,
    ) {
        // Ignora cargas que ya no corresponden a la vista actual de ESTA ventana.
        let expected = matches!(&ws.view, View::Loading { path: p } if *p == path);
        if !expected {
            tracing::debug!(cargado = %path.display(), "carga descartada: ya no es la vista Loading");
            return;
        }
        match &result {
            Ok(loader::LoadOutcome::Flat(img)) => tracing::debug!(
                ruta = %path.display(),
                ancho = img.width,
                alto = img.height,
                "editor: abre imagen plana"
            ),
            Ok(loader::LoadOutcome::Restored(_)) => {
                tracing::debug!(ruta = %path.display(), "editor: abre restaurada de sidecar");
            }
            Ok(loader::LoadOutcome::Design(_)) => {
                tracing::debug!(ruta = %path.display(), "editor: abre diseño");
            }
            Err(e) => tracing::debug!(ruta = %path.display(), error = %e, "editor: carga fallida"),
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
                            failed_path: Some(path),
                            failed_kind: None,
                        };
                    }
                }
            }
            Err(e) => {
                let kind = canvas_io::classify(&path);
                ws.view = View::Welcome {
                    error: Some(format!(
                        "Could not open \"{}\": {}",
                        path.display(),
                        crate::deck::failure_notice(Some(kind), &e.to_string())
                    )),
                    failed_path: Some(path),
                    failed_kind: Some(kind),
                };
            }
        }
        self.sync_title(ctx, ws);
    }

    pub(super) fn on_image_loaded_for_layer(
        ws: &mut Workspace,
        dest: loader::ImageInsertDest,
        path: PathBuf,
        result: Result<canvas_io::LoadedImage, canvas_io::IoError>,
    ) {
        if !dest.is_current(
            ws.deck.generation,
            ws.deck.slots.get(ws.deck.active).map(|s| s.id),
        ) {
            return;
        }
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
        ws: &mut Workspace,
        dest: loader::ImageInsertDest,
        layer: canvas_core::LayerId,
        label: String,
        source_path: Option<PathBuf>,
        result: Result<canvas_io::LoadedImage, canvas_io::IoError>,
    ) {
        if !dest.is_current(
            ws.deck.generation,
            ws.deck.slots.get(ws.deck.active).map(|s| s.id),
        ) {
            return;
        }
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
        result: Result<Box<deck::SlotDoc>, canvas_io::IoError>,
        ctx: &egui::Context,
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
                    // `SlotContent::Failed` guarda el mensaje listo para la
                    // UI más el tipo clasificado (una lectura de
                    // cabecera/CRC en el hilo de UI: solo llega aquí una vez
                    // por archivo fallido). Se aplana el error tipado aquí.
                    let kind = result.as_ref().err().map(|_| canvas_io::classify(&path));
                    let content = result.map_or_else(
                        |e| deck::SlotContent::Failed {
                            message: e.to_string(),
                            kind,
                        },
                        deck::SlotContent::Ready,
                    );
                    if let Some(slot) = ws.deck.slots.get_mut(idx) {
                        slot.content = content;
                    }
                    if let Some(kind) = kind {
                        if canvas_io::is_image_file(&path) {
                            super::repair::maybe_auto_handle(ws, &self.settings, &path, kind, ctx);
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_local_image_results_do_not_edit_another_document() {
        for stale_generation in [false, true] {
            let mut ws = Workspace::new(egui::ViewportId::ROOT);
            ws.deck = deck::Deck::single("current.png".into());
            ws.view = View::Editor(Box::new(editor::EditorState::new_blank_image(20.0, 20.0)));
            let dest = loader::ImageInsertDest {
                generation: if stale_generation {
                    ws.deck.generation + 1
                } else {
                    ws.deck.generation
                },
                slot_id: if stale_generation {
                    ws.deck.slots[0].id
                } else {
                    ws.deck.slots[0].id + 1
                },
            };
            let before = if let View::Editor(state) = &ws.view {
                state.doc.clone()
            } else {
                unreachable!()
            };
            AppInner::on_image_loaded_for_layer(
                &mut ws,
                dest,
                "incoming.png".into(),
                Ok(canvas_io::LoadedImage {
                    rgba: vec![255; 4],
                    width: 1,
                    height: 1,
                }),
            );
            AppInner::on_image_loaded_for_replace(
                &mut ws,
                dest,
                canvas_core::LayerId::from_raw(1),
                "incoming".into(),
                None,
                Err(canvas_io::IoError::Message {
                    message: "stale failure".into(),
                }),
            );
            let View::Editor(state) = &ws.view else {
                unreachable!()
            };
            assert_eq!(state.doc, before);
            assert!(state.save_error.is_none());
        }
    }

    /// Abrir el elemento central de una semilla de 3 conserva la baraja con
    /// el clicado activo (antes, `on_image_loaded` la tiraba y dejaba una
    /// sola ranura). Requiere GPU real (`RenderState` headless sin
    /// superficie): `#[ignore]` para que no falle en CI sin adaptador, igual
    /// que `canvas-render/tests/gpu_bake.rs`. Se corre con
    /// `cargo test -p canvas-app -- --ignored`.
    #[test]
    #[ignore]
    fn opening_the_middle_item_keeps_the_seeded_deck() {
        let rs = pollster::block_on(eframe::egui_wgpu::RenderState::create(
            &eframe::egui_wgpu::WgpuConfiguration::default(),
            &eframe::egui_wgpu::wgpu::Instance::new(
                eframe::egui_wgpu::wgpu::InstanceDescriptor::new_without_display_handle(),
            ),
            None,
            eframe::egui_wgpu::RendererOptions::default(),
        ))
        .expect("RenderState headless para el test");
        let renderer = canvas_render::CanvasRenderer::new(&rs.device).expect("renderer compartido");
        let (shell_tx, shell_rx) = std::sync::mpsc::channel();
        // La imagen mide lo mismo que `last_page_size` por defecto: abrirla
        // no dispara `save_in_background` (sin escrituras a disco reales).
        let settings = crate::settings::AppSettings {
            last_page_size: (1920.0, 1080.0),
            ..crate::settings::AppSettings::default()
        };
        let mut app = AppInner {
            workspaces: Vec::new(),
            me: None,
            renderer,
            rs,
            settings,
            thumb_cache: None,
            shell_status: String::new(),
            menu_mirror: crate::app::MenuMirror::default(),
            applied_theme: None,
            switcher: crate::app::switcher::SwitcherState::default(),
            focused: 0,
            shell_rx,
            shell_tx,
            next_ws_id: 1,
            last_workspaces_save: None,
            workspaces_dirty: false,
            pending_focus: None,
            atlas_regs: 0,
            atlas_reups: 0,
            atlas_log_frames: 0,
        };
        let dir = PathBuf::from("seed-test");
        let paths: Vec<PathBuf> = ["a.png", "b.png", "c.png"]
            .into_iter()
            .map(|n| dir.join(n))
            .collect();
        let seed = deck::DeckSeed {
            folder: dir,
            sort: crate::settings::GallerySort::Name,
            items: paths
                .iter()
                .map(|p| deck::SeedItem {
                    path: p.clone(),
                    name: p.file_name().unwrap().to_string_lossy().into_owned(),
                    kind: crate::gallery::ItemKind::Image,
                    mtime: None,
                    thumb: None,
                    thumb_failed: false,
                })
                .collect(),
        };
        let mut ws = Workspace::new(egui::ViewportId::ROOT);
        ws.deck_ops.pending_deck = Some(seed);
        let open = paths[1].clone();
        ws.view = View::Loading { path: open.clone() };
        let ctx = egui::Context::default();
        let img = canvas_io::LoadedImage {
            rgba: vec![128u8; 1920 * 1080 * 4],
            width: 1920,
            height: 1080,
        };
        app.on_image_loaded(
            &mut ws,
            open.clone(),
            Ok(loader::LoadOutcome::Flat(img)),
            canvas_io::ImageMetadata::default(),
            &ctx,
        );
        assert_eq!(ws.deck.slots.len(), 3, "la semilla sobrevive a la apertura");
        assert_eq!(ws.deck.slots[ws.deck.active].path, open);
        let View::Editor(state) = &ws.view else {
            unreachable!("debe abrir el editor");
        };
        assert_eq!(state.doc.source_path.as_deref(), Some(open.as_path()));
    }
}
