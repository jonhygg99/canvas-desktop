//! Los tres modales del editor: aviso de sobrescritura destructiva, aviso de
//! «este formato no se puede sobrescribir» (SVG/GIF) y el dialogo de
//! exportacion.

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::editor;

use super::super::super::frame::EditorFrame;
use super::super::super::persistence::SaveContext;
use super::super::super::ui_modals::{
    discard_raster_modal_ui, export_flow_ui, low_memory_modal_ui, overwrite_modal_ui,
    readonly_modal_ui,
};

pub(super) fn show_modals(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
) {
    // Modal de aviso de sobrescritura destructiva.
    let mut sctx = SaveContext {
        renderer: f.renderer,
        rs,
        tx: f.tx,
        ctx,
        ignore_fs_events_until: f.ignore_fs_events_until,
        scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
    };
    overwrite_modal_ui(state, &mut sctx, f.deck, f.save, f.settings);

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

    // Aviso de guardar un raster SIN capas de imagen (tras borrar la última
    // foto): el modal de sobrescritura puede encadenarlo (Choice::Overwrite),
    // y el guardado directo lo fija en `save_flow`. Va DESPUÉS de
    // `overwrite_modal_ui` para que, si ambos aplican, el usuario vea primero
    // el de sobrescritura y después este, más específico.
    discard_raster_modal_ui(state, f.save, &mut sctx, f.deck, f.settings);

    // Aviso de poca RAM antes de «Save all» masivo. Préstamos disjuntos de
    // `sctx` (que lleva renderer/tx/ignore_fs_events_until): aquí se usan
    // `f.deck` y `f.save`, que no están en él.
    low_memory_modal_ui(state, f.deck, f.save, ctx);

    // Diálogo de exportación. `sctx` ya está prestado arriba para el
    // modal de sobrescritura; se reutiliza aquí porque los modales son
    // mutuamente excluyentes (no se muestran a la vez). `f.deck` viaja como
    // préstamo corto para que el gate de RAM crítica pueda evictar la caché
    // propia antes de abortar.
    export_flow_ui(state, &mut sctx, f.deck, f.export);
}
