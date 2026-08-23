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
        &mut f.save.overwrite_prompt,
        &mut f.save.overwrite_confirmed,
        &mut f.save.overwrite_dont_ask,
        &mut f.save.close_after_save,
        &mut f.save.after_save,
        f.ignore_fs_events_until,
    );

    // Modal para SVG/GIF: no se pueden sobrescribir, se explica por qué y
    // se ofrece «Save as…» en su lugar.
    readonly_modal_ui(
        state,
        f.tx,
        ctx,
        &mut f.save.readonly_prompt,
        &mut f.save.close_after_save,
        &mut f.save.after_save,
    );

    // Diálogo de exportación.
    export_flow_ui(
        state,
        f.renderer,
        rs,
        f.tx,
        ctx,
        &mut f.export.export_dialog,
        &mut f.export.pending_export_settings,
        &mut f.export.pending_export,
    );
}
