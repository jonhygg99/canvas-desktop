//! Descargas con yt-dlp: panel del sidebar izquierdo.

pub mod api;
mod clip_cards;
mod clip_preview;
pub mod edit;
mod frames_cache;
pub mod panel;
pub mod state;

pub use edit::{edit_window_ui, open_pending_edit, retry_pending_frames, VideoAccept};
pub(crate) use frames_cache::{sweep_stale_frame_dirs, CachedFrames};
pub use panel::panel_ui;
pub use state::Panel;
