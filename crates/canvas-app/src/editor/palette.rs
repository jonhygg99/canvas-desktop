//! Buscar comandos por nombre; Enter ejecuta, flechas navegan, Escape cierra.
use super::{
    palette_actions::{Action, ACTIONS},
    EditorState,
};
use eframe::egui;

#[derive(Clone, Default)]
struct Search {
    query: String,
    selected: usize,
    focus: bool,
}
fn key() -> egui::Id {
    egui::Id::new("command_palette")
}
pub(crate) fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Search>(key()).is_some())
}
pub(crate) fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(
            key(),
            Search {
                focus: true,
                ..Default::default()
            },
        )
    });
}
pub(crate) fn button(state: &EditorState, ui: &mut egui::Ui) {
    if ui
        .add_enabled(
            state.is_idle() && state.framing.is_none(),
            egui::Button::new(crate::i18n::tr("Commands")),
        )
        .on_hover_text(format!("{}+Shift+P", crate::i18n::command()))
        .clicked()
    {
        open(ui.ctx());
    }
}
pub(crate) fn show(state: &mut EditorState, ctx: &egui::Context) {
    let Some(mut search) = ctx.data(|d| d.get_temp::<Search>(key())) else {
        return;
    };
    let mut open = true;
    let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let down = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
    let up = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
    let mut chosen = None;
    egui::Window::new(crate::i18n::tr("Commands"))
        .id(key())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(400.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            chosen = contents(state, ui, &mut search, [enter, down, up]);
        });
    if let Some(action) = chosen {
        ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("command_palette_query")));
        action.execute(state);
    } else if open && !escape {
        ctx.data_mut(|d| d.insert_temp(key(), search));
        return;
    }
    ctx.data_mut(|d| d.remove::<Search>(key()));
}
fn contents(
    state: &EditorState,
    ui: &mut egui::Ui,
    search: &mut Search,
    keys: [bool; 3],
) -> Option<Action> {
    let [enter, down, up] = keys;
    let mut chosen = None;
    let field = ui.add(
        egui::TextEdit::singleline(&mut search.query)
            .id(egui::Id::new("command_palette_query"))
            .hint_text(crate::i18n::tr("Search commands"))
            .desired_width(f32::INFINITY),
    );
    if search.focus {
        field.request_focus();
        search.focus = false;
    }
    if field.changed() {
        search.selected = 0;
    }
    let matches = filtered(&search.query);
    search.selected = search.selected.min(matches.len().saturating_sub(1));
    if down && !matches.is_empty() {
        search.selected = (search.selected + 1) % matches.len();
    }
    if up && !matches.is_empty() {
        search.selected = (search.selected + matches.len() - 1) % matches.len();
    }
    if enter {
        chosen = matches
            .get(search.selected)
            .copied()
            .filter(|a| a.enabled(state));
    }
    chosen = result_rows(state, ui, search, &matches, down || up).or(chosen);
    ui.weak(crate::i18n::tr("↑/↓: choose · Enter: run · Esc: close"));
    chosen
}

fn result_rows(
    state: &EditorState,
    ui: &mut egui::Ui,
    search: &Search,
    matches: &[Action],
    navigating: bool,
) -> Option<Action> {
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .max_height(300.0)
        .show(ui, |ui| {
            for (index, action) in matches.iter().enumerate() {
                let shortcut = action.shortcut();
                let label = if shortcut.is_empty() {
                    crate::i18n::tr(action.label()).to_owned()
                } else {
                    format!(
                        "{}    {}{shortcut}",
                        crate::i18n::tr(action.label()),
                        crate::i18n::command()
                    )
                };
                let response = ui.add_enabled(
                    action.enabled(state),
                    egui::Button::selectable(index == search.selected, label),
                );
                if navigating && index == search.selected {
                    response.scroll_to_me(Some(egui::Align::Center));
                }
                if response.clicked() {
                    chosen = Some(*action);
                }
            }
            if matches.is_empty() {
                ui.weak(crate::i18n::tr("No matching commands"));
            }
        });
    chosen
}

fn filtered(query: &str) -> Vec<Action> {
    let query = query.trim().to_lowercase();
    ACTIONS
        .iter()
        .copied()
        .filter(|a| {
            a.label().to_lowercase().contains(&query)
                || crate::i18n::tr(a.label()).to_lowercase().contains(&query)
        })
        .collect()
}

pub(crate) fn context_help(state: &EditorState) -> String {
    let command = crate::i18n::command();
    if state.inline_text.is_some() {
        format!(
            "{command}+Enter: {} · Esc: {}",
            crate::i18n::tr("Confirm text"),
            crate::i18n::tr("Cancel")
        )
    } else if state.insert_tool.is_some() {
        crate::i18n::tr("Click to place · Drag to choose size · Esc: cancel").to_owned()
    } else if state.selection.is_empty() {
        format!(
            "{} · {command}+Shift+P: {}",
            crate::i18n::tr("Drag empty space to select · Alt+click: overlapping layers"),
            crate::i18n::tr("Commands")
        )
    } else {
        format!(
            "{} · {command}+Alt+{} · {command}+Shift+D: {}",
            crate::i18n::tr("Arrows: 1 px · Shift+arrows: 10 px · Shift+drag: constrain"),
            crate::i18n::tr("drag: duplicate"),
            crate::i18n::tr("Repeat duplication")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn searching_and_enter_execute_the_command_without_global_shortcuts() {
        let (_, mut state, a, _) = crate::editor::selection_gesture_tests::fixture();
        state.selection.set(Some(a));
        let ctx = egui::Context::default();
        open(&ctx);
        for events in [
            vec![],
            vec![egui::Event::Text("duplicate".into())],
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_ui| {
                    state.handle_shortcuts(&ctx, false, false);
                    show(&mut state, &ctx);
                },
            );
        }
        assert_eq!(state.doc.page().unwrap().layers.len(), 3);
        assert!(!is_open(&ctx));
        assert_eq!(state.history.undo_depth(), 1);
    }
    #[test]
    fn palette_actions_respect_locked_layers_and_are_undoable() {
        let (_, mut state, a, _) = crate::editor::selection_gesture_tests::fixture();
        state.selection.set(Some(a));
        state.doc.layer_mut(a).unwrap().locked = true;
        Action::Duplicate.execute(&mut state);
        assert_eq!(state.doc.page().unwrap().layers.len(), 2);
        state.doc.layer_mut(a).unwrap().locked = false;
        Action::Duplicate.execute(&mut state);
        assert_eq!(state.doc.page().unwrap().layers.len(), 3);
        Action::Undo.execute(&mut state);
        assert_eq!(state.doc.page().unwrap().layers.len(), 2);
    }
    #[test]
    fn translated_search_finds_commands() {
        crate::i18n::set_language(crate::i18n::Language::Spanish);
        assert!(filtered("duplicar").contains(&Action::Duplicate));
        assert!(filtered(" DUPLICATE ").contains(&Action::Duplicate));
        assert!(filtered("missing-command").is_empty());
        crate::i18n::set_language(crate::i18n::Language::English);
    }
}
