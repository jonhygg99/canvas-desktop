//! Respuestas del hilo de reparación de PNG: el hermano `*-reparado`
//! recién escrito, o el motivo por el que no se pudo reparar.

use std::path::PathBuf;

use eframe::egui;

use crate::loader;

use super::super::{AppInner, View, Workspace};

/// Automáticos opt-in tras un fallo de carga/muestra: repara o cuarentena
/// según el tipo y los ajustes, una sola vez por (ruta, tamaño, mtime) y
/// nunca con RAM crítica. Llamar SOLO con archivos de imagen (las
/// miniaturas de diseños `.canvas` y los vídeos van por otro camino).
pub(super) fn maybe_auto_handle(
    ws: &mut Workspace,
    settings: &crate::settings::AppSettings,
    path: &PathBuf,
    kind: canvas_io::CorruptionKind,
    ctx: &egui::Context,
) {
    if deck_low_memory() {
        return;
    }
    let action = loader::auto_action_for(
        kind,
        settings.auto_repair_png,
        settings.quarantine_unreadable,
    );
    let Some(action) = action else {
        return;
    };
    let key = std::fs::metadata(path)
        .ok()
        .map(|m| (path.clone(), m.len(), m.modified().ok()));
    let Some(key) = key else {
        return;
    };
    if !ws.auto_handled.insert(key) {
        return;
    }
    match action {
        loader::AutoRepairAction::Repair => {
            loader::spawn_repair_png(path.clone(), ws.tx.clone(), ctx.clone());
        }
        loader::AutoRepairAction::Quarantine => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Quarantine { path: path.clone() },
                false,
                ws.tx.clone(),
                ctx.clone(),
            );
        }
    }
}

/// ¿La reparación en fondo debe esperar? Misma puerta que pausa la
/// precarga de la baraja (`deck::loading`): con RAM crítica ni se intenta.
fn deck_low_memory() -> bool {
    crate::deck::is_critical_free_ram(crate::deck::free_ram_bytes())
}

impl AppInner {
    pub(super) fn on_png_repaired(
        &mut self,
        ws: &mut Workspace,
        original: PathBuf,
        repaired: PathBuf,
        ctx: &egui::Context,
    ) {
        let folder = repaired
            .parent()
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        tracing::info!(
            original = %original.display(),
            repaired = %repaired.display(),
            "reparación lista en la UI"
        );
        // Desde el callejón de bienvenida, la reparación culmina abriendo
        // el archivo reparado (en galería/baraja basta con que aparezca al
        // reescanear).
        if let View::Welcome { failed_path, .. } = &ws.view {
            if *failed_path == Some(original.clone()) {
                loader::spawn_load_image(
                    repaired.clone(),
                    self.settings.sidecar_default,
                    ws.tx.clone(),
                    ctx.clone(),
                );
                ws.view = View::Loading { path: repaired };
                return;
            }
        }
        if matches!(&ws.view, View::Gallery(g) if g.is_affected_by(&folder)) {
            if let View::Gallery(g) = &mut ws.view {
                g.selected = Some(repaired.clone());
            }
            self.rescan_gallery(ws, ctx);
        }
        if ws.deck.folder.as_deref() == Some(folder.as_path()) {
            loader::spawn_gallery_scan(
                folder,
                self.thumb_cache.clone(),
                ws.tx.clone(),
                ctx.clone(),
            );
        }
    }

    pub(super) fn on_repair_failed(
        &mut self,
        ws: &mut Workspace,
        original: PathBuf,
        kind: canvas_io::CorruptionKind,
        message: String,
    ) {
        tracing::warn!(
            original = %original.display(),
            ?kind,
            message = %message,
            "reparación imposible"
        );
        let folder = original
            .parent()
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        if let View::Welcome {
            error, failed_path, ..
        } = &mut ws.view
        {
            if *failed_path == Some(original.clone()) {
                *error = Some(message.clone());
                return;
            }
        }
        if let View::Gallery(g) = &mut ws.view {
            if g.is_affected_by(&folder) {
                g.op_error = Some(message);
            }
        }
    }
}
