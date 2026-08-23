//! Los tres modales del editor: aviso de sobrescritura destructiva, aviso de
//! «este formato no se puede sobrescribir» (SVG/GIF) y el dialogo de
//! exportacion.

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::editor;

use super::super::super::frame::EditorFrame;
use super::super::super::ui_modals::{export_flow_ui, overwrite_modal_ui, readonly_modal_ui};

pub(super) fn show_modals(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
) {
    // Modal de aviso de sobrescritura destructiva.
    overwrite_modal_ui(
        state,
        f.renderer,
        rs,
        f.tx,
        ctx,
        f.settings,
        f.overwrite_prompt,
        f.overwrite_confirmed,
        f.overwrite_dont_ask,
        f.close_after_save,
        f.after_save,
        f.ignore_fs_events_until,
    );

    // Modal para SVG/GIF: no se pueden sobrescribir, se explica por qué y
    // se ofrece «Save as…» en su lugar.
    readonly_modal_ui(
        state,
        f.tx,
        ctx,
        f.readonly_prompt,
        f.close_after_save,
        f.after_save,
    );

    // Diálogo de exportación.
    export_flow_ui(
        state,
        f.renderer,
        rs,
        f.tx,
        ctx,
        f.export_dialog,
        f.pending_export_settings,
        f.pending_export,
    );
}
