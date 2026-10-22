use super::*;
use crate::deck;

fn app() -> AppInner {
    let rs = pollster::block_on(eframe::egui_wgpu::RenderState::create(
        &eframe::egui_wgpu::WgpuConfiguration::default(),
        &eframe::egui_wgpu::wgpu::Instance::new(
            eframe::egui_wgpu::wgpu::InstanceDescriptor::new_without_display_handle(),
        ),
        None,
        eframe::egui_wgpu::RendererOptions::default(),
    ))
    .expect("headless GPU");
    let renderer = canvas_render::CanvasRenderer::new(&rs.device).unwrap();
    let (shell_tx, shell_rx) = std::sync::mpsc::channel();
    AppInner {
        workspaces: Vec::new(),
        me: None,
        renderer,
        rs,
        settings: crate::settings::AppSettings::default(),
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
    }
}

fn workspace() -> Workspace {
    let mut ws = Workspace::new(egui::ViewportId::ROOT);
    ws.deck = deck::Deck::new_design((96.0, 64.0));
    let mut state = editor::EditorState::new_blank_image(96.0, 64.0);
    state.doc.source_path = Some("source.mp4".into());
    state
        .doc
        .add_layer(
            "video",
            canvas_core::Transform::new(16.0, 8.0, 64.0, 48.0),
            canvas_core::LayerContent::Video(canvas_core::VideoContent {
                source_path: Some("source.mp4".into()),
                natural_width: 64,
                natural_height: 48,
                crop: None,
                duration_secs: Some(10.0),
                poster_time: 2.0,
                trim_start: 2.0,
                trim_end: Some(7.5),
            }),
        )
        .unwrap();
    state.history.mark_unsaved();
    ws.view = View::Editor(Box::new(state));
    ws
}

fn capture(ws: &mut Workspace, dst: &str) -> loader::VideoSaveDone {
    let View::Editor(state) = &mut ws.view else {
        panic!("editor")
    };
    state.saving = true;
    let capture = (state.history.undo_depth(), state.history.revision());
    state.saving_capture = Some(capture);
    loader::VideoSaveDone {
        target: loader::ImageInsertDest {
            generation: ws.deck.generation(),
            slot_id: ws.deck.slots[ws.deck.active].id,
        },
        capture,
        dst: dst.into(),
        result: Ok(canvas_io::VideoRenderInfo {
            duration_secs: 5.5,
            width: 96,
            height: 64,
        }),
        links: vec![("source.mp4".into(), ".download/source.mp4".into())],
    }
}

#[test]
fn video_callbacks_preserve_edits_reject_stale_results_and_drain_save_all() {
    let mut app = app();
    let ctx = egui::Context::default();
    let mut ws = workspace();
    let mut next = None;
    // Respuesta de otra generación o id: ni limpia ni muta el activo.
    for wrong_generation in [false, true] {
        let mut done = capture(&mut ws, "out.mp4");
        if wrong_generation {
            done.target.generation += 1;
        } else {
            done.target.slot_id += 1;
        }
        let View::Editor(state) = &ws.view else {
            panic!("editor")
        };
        let before = state.doc.clone();
        app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);
        let View::Editor(state) = &ws.view else {
            panic!("editor")
        };
        assert_eq!(state.doc, before);
        assert!(state.saving && state.is_dirty());
    }
    // El preview de vídeo escribe el reloj de reproducción en el documento en
    // CADA fotograma, sin comando de historial. Eso NO es una edición: antes,
    // comparar documentos enteros hacía que el clip se viera «editado durante
    // el guardado» para siempre y el autoguardado lo re-renderizara cada
    // `VIDEO_QUIET`. La captura del historial es el único juez.
    let done = capture(&mut ws, "out.mp4");
    ws.save.after_save = Some(Nav::CloseProject);
    ws.save.save_all_queue = vec![ws.deck.slots[0].id];
    let View::Editor(state) = &mut ws.view else {
        panic!("editor")
    };
    let layer = &mut state.doc.page_mut().unwrap().layers[0];
    let canvas_core::LayerContent::Video(video) = &mut layer.content else {
        panic!("video")
    };
    video.poster_time = 4.25;
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);
    let View::Editor(state) = &ws.view else {
        panic!("editor")
    };
    let layer = &state.doc.page().unwrap().layers[0];
    let canvas_core::LayerContent::Video(video) = &layer.content else {
        panic!("video")
    };
    assert_eq!(video.poster_time, 4.25, "el preview no se pisa al guardar");
    assert!(
        !state.saving && !state.is_dirty(),
        "el reloj de reproducción no deja el clip sucio"
    );
    assert!(ws.save.save_all_queue.is_empty());
    assert!(matches!(next, Some(Nav::CloseProject)));

    // Una edición REAL durante el render (apilada en History) sí deja el
    // lienzo pendiente: el archivo escrito no la contiene.
    ws = workspace();
    ws.save.after_save = Some(Nav::CloseProject);
    next = None;
    let done = capture(&mut ws, "out.mp4");
    let View::Editor(state) = &mut ws.view else {
        panic!("editor")
    };
    state.history.push_applied(Box::new(canvas_core::Rename {
        layer: canvas_core::LayerId::from_raw(1),
        before: "video".into(),
        after: "video (edited)".into(),
    }));
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);
    let View::Editor(state) = &ws.view else {
        panic!("editor")
    };
    assert!(state.is_dirty() && !state.saving);
    assert!(state.save_error.is_some(), "se explica la edición en vuelo");
    assert!(next.is_none() && ws.save.after_save.is_none());

    // Dos lienzos: el primero no ejecuta la salida; el segundo sí.
    ws = workspace();
    let second = ws.deck.push_placeholder((96.0, 64.0), "mp4").unwrap();
    if let deck::SlotContent::Ready(doc) = &mut ws.deck.slots[second].content {
        doc.history.mark_unsaved();
    }
    ws.save.save_all_queue = ws.deck.slots.iter().map(|s| s.id).collect();
    ws.save.after_save = Some(Nav::CloseProject);
    let done = capture(&mut ws, "first.mp4");
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);
    assert_eq!(ws.save.save_all_queue.len(), 1);
    assert!(next.is_none());
    let View::Editor(state) = &mut ws.view else {
        panic!("editor")
    };
    ws.deck.jump_to = Some(second);
    assert!(matches!(
        deck::apply_jump(&mut ws.deck, state),
        deck::JumpOutcome::Applied
    ));
    let done = capture(&mut ws, "second.mp4");
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);
    assert!(ws.save.save_all_queue.is_empty());
    assert!(matches!(next, Some(Nav::CloseProject)));
    assert!(ws.ignore_fs_events_until.is_some());

    // Crear mientras guarda queda en cola, no extrae el documento.
    ws = workspace();
    let _done = capture(&mut ws, "out.mp4");
    app.request_nav(&mut ws, Nav::OpenVideos { accepts: vec![] }, &ctx);
    assert_eq!(ws.deck.slots.len(), 1);
    assert_eq!(ws.deferred_nav.len(), 1);
}

#[test]
fn rendered_save_relinks_sibling_canvases_sharing_the_temp_source() {
    let mut app = app();
    let ctx = egui::Context::default();
    let mut ws = workspace();
    // Segundo lienzo del mismo temporal, guardado en la baraja.
    let mut sibling = editor::EditorState::new_blank_image(96.0, 64.0);
    sibling
        .doc
        .add_layer(
            "video",
            canvas_core::Transform::new(16.0, 8.0, 64.0, 48.0),
            canvas_core::LayerContent::Video(canvas_core::VideoContent {
                source_path: Some("source.mp4".into()),
                natural_width: 64,
                natural_height: 48,
                crop: None,
                duration_secs: Some(10.0),
                poster_time: 2.0,
                trim_start: 1.0,
                trim_end: Some(4.0),
            }),
        )
        .unwrap();
    let idx = ws.deck.push_placeholder((96.0, 64.0), "mp4").unwrap();
    ws.deck.slots[idx].content = deck::SlotContent::Ready(Box::new(sibling.take_slot()));
    let mut next = None;
    let done = capture(&mut ws, "out.mp4");
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);
    let deck::SlotContent::Ready(doc) = &ws.deck.slots[idx].content else {
        panic!("ready")
    };
    let layer = &doc.doc.page().unwrap().layers[0];
    let canvas_core::LayerContent::Video(video) = &layer.content else {
        panic!("video")
    };
    // El hermano bebe de la misma copia duradera: limpiar Download ya no
    // lo deja sin fuente aunque sus bytes vivan en el proyecto.
    assert_eq!(
        video.source_path.as_deref(),
        Some(std::path::Path::new(".download/source.mp4"))
    );
    assert_eq!((video.trim_start, video.trim_end), (1.0, Some(4.0)));
}

/// Documento de clip para una ranura de fondo: vídeo + fondo, como el que
/// produce «Add clips».
fn sibling_clip(page: (f64, f64)) -> editor::EditorState {
    let mut state = editor::EditorState::new_blank_image(page.0, page.1);
    state.doc.source_path = Some("source.mp4".into());
    state
        .doc
        .add_layer(
            "video",
            canvas_core::Transform::new(0.0, 0.0, page.0, page.1),
            canvas_core::LayerContent::Video(canvas_core::VideoContent {
                source_path: Some("source.mp4".into()),
                natural_width: 64,
                natural_height: 48,
                crop: None,
                duration_secs: Some(10.0),
                poster_time: 1.0,
                trim_start: 1.0,
                trim_end: Some(6.0),
            }),
        )
        .unwrap();
    state
}

/// Un render de FONDO de la cola de clips aplica el mp4 a SU ranura y no toca
/// el documento en pantalla, ni la navegación diferida, ni el estado de
/// guardado del activo.
#[test]
fn a_background_clip_render_adopts_its_slot_and_leaves_the_active_canvas_alone() {
    let mut app = app();
    let ctx = egui::Context::default();
    let mut ws = workspace();
    let tmp = tempfile::tempdir().unwrap();
    let dst = tmp.path().join("clip-01.mp4");
    std::fs::write(&dst, b"x").unwrap();
    let idx = ws.deck.push_placeholder((96.0, 64.0), "mp4").unwrap();
    let slot_id = ws.deck.slots[idx].id;
    let mut slot = sibling_clip((96.0, 64.0)).take_slot();
    slot.history.mark_unsaved();
    let capture = (slot.history.undo_depth(), slot.history.revision());
    slot.saving = true;
    slot.saving_capture = Some(capture);
    ws.deck.slots[idx].content = deck::SlotContent::Ready(Box::new(slot));

    let View::Editor(state) = &ws.view else {
        panic!("editor")
    };
    let active_before = state.doc.clone();
    let mut next = None;
    let done = loader::VideoSaveDone {
        target: loader::ImageInsertDest {
            generation: ws.deck.generation(),
            slot_id,
        },
        capture,
        dst: dst.clone(),
        result: Ok(canvas_io::VideoRenderInfo {
            duration_secs: 5.0,
            width: 96,
            height: 64,
        }),
        links: vec![("source.mp4".into(), ".download/source.mp4".into())],
    };
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);

    let View::Editor(state) = &ws.view else {
        panic!("editor")
    };
    // El único cambio permitido en el activo es el reenlace de fuentes (todos
    // los recortes del temporal comparten la copia duradera); su geometría,
    // trim y efectos siguen exactamente como estaban.
    let live = &state.doc.page().unwrap().layers[0];
    let before = &active_before.page().unwrap().layers[0];
    assert_eq!(live.transform, before.transform);
    assert_eq!(live.effects, before.effects);
    let canvas_core::LayerContent::Video(live) = &live.content else {
        panic!("video")
    };
    let canvas_core::LayerContent::Video(before) = &before.content else {
        panic!("video")
    };
    assert_eq!(live.trim_start, before.trim_start);
    assert_eq!(live.trim_end, before.trim_end);
    assert_eq!(live.poster_time, before.poster_time);
    assert_eq!(
        live.source_path.as_deref(),
        Some(std::path::Path::new(".download/source.mp4"))
    );
    assert_eq!(state.doc.source_path, active_before.source_path);
    assert!(!state.saving, "el activo no estaba guardando");
    assert!(next.is_none(), "un render de fondo no navega");
    assert!(
        !ws.save.writes.busy(ws.deck.generation()),
        "la cola queda libre"
    );

    let slot = &ws.deck.slots[idx];
    assert!(!slot.is_placeholder);
    assert_eq!(slot.path, dst);
    assert_eq!(slot.name, "clip-01.mp4");
    assert_eq!(slot.kind, crate::gallery::ItemKind::Video);
    let deck::SlotContent::Ready(doc) = &slot.content else {
        panic!("ready")
    };
    assert!(!doc.saving);
    assert!(!doc.history.is_dirty(), "el archivo contiene esta revisión");
    assert!(doc.save_error.is_none());
    assert_eq!(doc.doc.source_path.as_deref(), Some(dst.as_path()));
    let canvas_core::LayerContent::Video(video) = &doc.doc.page().unwrap().layers[0].content else {
        panic!("video")
    };
    assert_eq!(
        video.source_path.as_deref(),
        Some(std::path::Path::new(".download/source.mp4"))
    );
}

/// Un render de fondo que falla deja la ranura sucia, con SU error y sin
/// inventar un archivo: el siguiente intento (autoguardado o `Ctrl+S`) la
/// reencola. El fallo nunca envenena el activo ni la cola.
#[test]
fn a_failed_background_clip_stays_dirty_with_its_own_error() {
    let mut app = app();
    let ctx = egui::Context::default();
    let mut ws = workspace();
    let idx = ws.deck.push_placeholder((96.0, 64.0), "mp4").unwrap();
    let slot_id = ws.deck.slots[idx].id;
    let mut slot = sibling_clip((96.0, 64.0)).take_slot();
    slot.history.mark_unsaved();
    let capture = (slot.history.undo_depth(), slot.history.revision());
    slot.saving = true;
    slot.saving_capture = Some(capture);
    slot.save_error = Some("stale".into());
    ws.deck.slots[idx].content = deck::SlotContent::Ready(Box::new(slot));

    let mut next = None;
    let done = loader::VideoSaveDone {
        target: loader::ImageInsertDest {
            generation: ws.deck.generation(),
            slot_id,
        },
        capture,
        dst: std::path::PathBuf::from("clip-01.mp4"),
        result: Err(canvas_io::IoError::Message {
            message: "ffmpeg died".into(),
        }),
        links: Vec::new(),
    };
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);

    let slot = &ws.deck.slots[idx];
    assert!(slot.is_placeholder, "un fallo no reserva el nombre");
    let deck::SlotContent::Ready(doc) = &slot.content else {
        panic!("ready")
    };
    assert!(!doc.saving);
    assert!(doc.history.is_dirty(), "sigue pendiente de guardado");
    let error = doc.save_error.as_deref().expect("error propio");
    assert!(error.contains("ffmpeg died"), "{error}");
    assert!(!ws.save.writes.busy(ws.deck.generation()));
    assert!(next.is_none());
}

/// La misma adopción vale para un lienzo de fondo RASTER: `on_saved` con el
/// ticket de una ranura que no está en pantalla escribe SU archivo, la marca
/// limpia y no toca el documento activo ni la navegación diferida.
#[test]
fn a_background_raster_save_lands_on_its_slot_and_leaves_the_active_doc_alone() {
    let mut app = app();
    let ctx = egui::Context::default();
    let mut ws = workspace();
    let tmp = tempfile::tempdir().unwrap();
    let dst = tmp.path().join("b.png");
    std::fs::write(&dst, b"x").unwrap();
    let idx = ws.deck.push_placeholder((96.0, 64.0), "png").unwrap();
    let slot_id = ws.deck.slots[idx].id;
    let mut background = editor::EditorState::new_blank_image(96.0, 64.0).take_slot();
    background.doc.source_path = Some(dst.clone());
    background.history.mark_unsaved();
    let capture = (
        background.history.undo_depth(),
        background.history.revision(),
    );
    ws.deck.slots[idx].content = deck::SlotContent::Ready(Box::new(background));

    let View::Editor(state) = &ws.view else {
        panic!("editor")
    };
    let active_before = state.doc.clone();
    let generation = ws.deck.generation();
    let mut next = None;
    app.on_saved(
        &mut ws,
        super::super::saved::SavedFile {
            path: dst.clone(),
            result: Ok(()),
            new_source: false,
            ticket: loader::SaveTicket {
                target: loader::ImageInsertDest {
                    generation,
                    slot_id,
                },
                capture,
            },
        },
        &ctx,
        &mut next,
    );

    let View::Editor(state) = &ws.view else {
        panic!("editor")
    };
    assert_eq!(state.doc, active_before, "el lienzo en pantalla no se toca");
    assert!(!state.saving);
    let deck::SlotContent::Ready(doc) = &ws.deck.slots[idx].content else {
        panic!("ready")
    };
    assert!(!doc.history.is_dirty(), "la ranura queda limpia");
    assert!(!doc.saving);
    assert_eq!(doc.doc.source_path.as_deref(), Some(dst.as_path()));
    assert!(!ws.deck.slots[idx].is_placeholder);
    assert!(next.is_none(), "una escritura de fondo no navega");
}

/// Un fallo de fondo deja el banner en SU ranura y no se reintenta hasta la
/// próxima edición (nada de un horneado por frame).
#[test]
fn a_failed_background_save_blocks_that_revision_only() {
    let mut app = app();
    let ctx = egui::Context::default();
    let mut ws = workspace();
    let idx = ws.deck.push_placeholder((96.0, 64.0), "png").unwrap();
    let slot_id = ws.deck.slots[idx].id;
    let mut background = editor::EditorState::new_blank_image(96.0, 64.0).take_slot();
    background.doc.source_path = Some("b.png".into());
    background.history.mark_unsaved();
    let capture = (
        background.history.undo_depth(),
        background.history.revision(),
    );
    ws.deck.slots[idx].content = deck::SlotContent::Ready(Box::new(background));

    let generation = ws.deck.generation();
    let mut next = None;
    app.on_saved(
        &mut ws,
        super::super::saved::SavedFile {
            path: "b.png".into(),
            result: Err(canvas_io::IoError::Message {
                message: "disk full".into(),
            }),
            new_source: false,
            ticket: loader::SaveTicket {
                target: loader::ImageInsertDest {
                    generation,
                    slot_id,
                },
                capture,
            },
        },
        &ctx,
        &mut next,
    );

    let deck::SlotContent::Ready(doc) = &ws.deck.slots[idx].content else {
        panic!("ready")
    };
    assert!(!doc.saving);
    assert!(doc.history.is_dirty(), "sigue pendiente de guardado");
    let error = doc.save_error.as_deref().expect("error propio");
    assert!(error.contains("disk full"), "{error}");
    assert_eq!(ws.save.blocked_writes.get(&slot_id), Some(&capture.1));
    assert!(next.is_none());
}
