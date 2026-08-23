//! Contenido de cada vista (`View::Welcome`/`Loading`/`Gallery`/`Editor`)
//! dentro del `CentralPanel` del frame.

mod editor;
mod gallery;
mod loading;
mod welcome;

pub(super) use editor::editor_view_ui;
pub(super) use gallery::gallery_view_ui;
pub(super) use loading::loading_view_ui;
pub(super) use welcome::welcome_view_ui;
