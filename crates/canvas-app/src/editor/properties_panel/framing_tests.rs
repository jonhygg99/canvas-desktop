use super::*;

#[test]
fn sidebar_button_enters_and_leaves_framing_without_changing_the_page() {
    let ctx = egui::Context::default();
    let mut state = EditorState::new_blank(1920.0, 1080.0);
    let original = serde_json::to_vec(&state.doc).unwrap();
    let frame = |events, state: &mut EditorState| {
        ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| framing_header_ui(state, ui),
        )
    };
    let click = |label: &str, state: &mut EditorState| {
        frame(vec![], state);
        let output = frame(vec![], state);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() / 2.0)
                }
                _ => None,
            })
            .expect("sidebar action rendered");
        frame(vec![egui::Event::PointerMoved(pos)], state);
        for pressed in [true, false] {
            frame(
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                state,
            );
        }
    };
    click("Framing 9:16", &mut state);
    assert!(state.framing_requested);
    state.framing = Some(crate::framing::Session::new(None, true));
    click("Normal view", &mut state);
    assert!(state.framing.as_ref().unwrap().closed);
    assert_eq!(serde_json::to_vec(&state.doc).unwrap(), original);
}
