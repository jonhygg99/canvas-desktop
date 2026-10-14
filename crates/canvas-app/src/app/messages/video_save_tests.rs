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
        document: state.doc.clone(),
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
    let done = capture(&mut ws, "out.mp4");
    ws.save.after_save = Some(Nav::CloseProject);
    ws.save.save_all_queue = vec![ws.deck.slots[0].id];
    let View::Editor(state) = &mut ws.view else {
        panic!("editor")
    };
    let layer = &mut state.doc.page_mut().unwrap().layers[0];
    layer.transform.x = 23.0;
    let canvas_core::LayerContent::Video(video) = &mut layer.content else {
        panic!("video")
    };
    video.trim_start = 3.0; // Cambio directo todavía sin consolidar en History.
    app.on_video_composition_rendered(&mut ws, done, &ctx, &mut next);
    let View::Editor(state) = &ws.view else {
        panic!("editor")
    };
    let layer = &state.doc.page().unwrap().layers[0];
    let canvas_core::LayerContent::Video(video) = &layer.content else {
        panic!("video")
    };
    assert_eq!(video.trim_start, 3.0);
    assert_eq!(layer.transform.x, 23.0);
    assert!(state.is_dirty() && !state.saving);
    assert!(next.is_none() && ws.save.after_save.is_none());
    assert!(ws.save.save_all_queue.is_empty());

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
