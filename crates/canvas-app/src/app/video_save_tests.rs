//! Regresiones de identidad de Download sin GPU ni medios del usuario.
use super::*;
use crate::app::video_canvas_names::rename_provisional;
use std::collections::HashSet;

fn accept(dir: &std::path::Path) -> VideoAccept {
    let poster = dir.join("poster.png");
    image::RgbaImage::from_pixel(16, 9, image::Rgba([30, 80, 160, 255]))
        .save(&poster)
        .unwrap();
    let path = dir.join("source.mp4");
    std::fs::write(&path, b"original download").unwrap();
    let mut accept = crate::ytdlp::edit::default_accept(path, "Clip".into(), (96.0, 64.0));
    accept.poster = poster;
    accept.video_size = Some((16.0, 9.0));
    accept
}

#[test]
fn ten_repeated_insertions_have_independent_names_ids_and_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let accept = accept(tmp.path());
    for folder in [false, true] {
        for format in [NewCanvasFormat::Canvas, NewCanvasFormat::Png] {
            let project = tempfile::tempdir().unwrap();
            let mut deck = deck::Deck::new_design((96.0, 64.0));
            if folder {
                deck.folder = Some(project.path().to_owned());
            }
            let mut state = editor::EditorState::new_blank(96.0, 64.0);
            for _ in 0..10 {
                let mut built = build_video_canvas(&accept, format).unwrap();
                let first = stash_and_push(
                    &mut deck,
                    state.take_slot(),
                    vec![(built.take_slot(), accept.size)],
                    std::slice::from_ref(&accept),
                    format,
                )
                .unwrap();
                let deck::SlotContent::Ready(doc) =
                    std::mem::replace(&mut deck.slots[first].content, deck::SlotContent::Active)
                else {
                    panic!("ready")
                };
                state.put_slot(*doc);
                deck.active = first;
                assert_eq!(state.file_name(), deck.slots[first].name);
                if format == NewCanvasFormat::Canvas && !folder {
                    assert!(state.doc.source_path.is_none());
                }
                assert_eq!(
                    state
                        .doc
                        .page()
                        .unwrap()
                        .layers
                        .iter()
                        .find_map(|l| match &l.content {
                            canvas_core::LayerContent::Video(v) => v.source_path.as_ref(),
                            _ => None,
                        }),
                    Some(&accept.path)
                );
            }
            let clips = &deck.slots[1..];
            assert_eq!(clips.iter().map(|s| s.id).collect::<HashSet<_>>().len(), 10);
            assert_eq!(
                clips.iter().map(|s| &s.name).collect::<HashSet<_>>().len(),
                10
            );
            assert_eq!(
                clips.iter().map(|s| &s.path).collect::<HashSet<_>>().len(),
                10
            );
            assert_eq!(std::fs::read_dir(project.path()).unwrap().count(), 0);
        }
    }
    assert_eq!(std::fs::read(&accept.path).unwrap(), b"original download");
}

#[test]
fn provisional_rename_never_renames_download_or_loses_the_canvas() {
    let tmp = tempfile::tempdir().unwrap();
    let accept = accept(tmp.path());
    let built = build_video_canvas(&accept, NewCanvasFormat::Png).unwrap();
    let (mut state, mut deck) = assemble_canvases(
        vec![built],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Png,
    );
    let before = state.doc.clone();
    let id = deck.slots[0].id;
    assert!(rename_provisional(&mut deck, &mut state, id, "renamed").unwrap());
    assert_eq!(deck.slots.len(), 1);
    assert_eq!(state.file_name(), "renamed.mp4");
    assert_eq!(state.doc, before);
    assert_eq!(std::fs::read(&accept.path).unwrap(), b"original download");
    assert!(rename_provisional(&mut deck, &mut state, id, "../bad").is_err());
}

#[test]
fn canvas_without_folder_has_no_video_write_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let accept = accept(tmp.path());
    let built = build_video_canvas(&accept, NewCanvasFormat::Canvas).unwrap();
    assert!(built.doc.source_path.is_none());
    let (state, deck) = assemble_canvases(
        vec![built],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Canvas,
    );
    assert!(state.doc.source_path.is_none());
    assert!(canvas_io::is_canvas_file(&deck.slots[0].path));
    assert_eq!(state.file_name(), deck.slots[0].name);
    assert!(canvas_io::write_design(&accept.path, &state.sidecar_payload()).is_err());
    assert_eq!(std::fs::read(&accept.path).unwrap(), b"original download");
}
