//! Descargas con yt-dlp: panel del sidebar izquierdo.

pub mod api;
pub mod edit;
pub mod panel;
pub mod state;

pub use edit::{edit_window_ui, open_pending_edit, retry_pending_frames, VideoAccept};
pub use panel::panel_ui;
pub use state::Panel;
